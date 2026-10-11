import type { IconifyIcon } from "@iconify/types";
import dartIcon from "@iconify-icons/logos/dart";
import cIcon from "@iconify-icons/logos/c";
import cppIcon from "@iconify-icons/logos/c-plusplus";
import kotlinIcon from "@iconify-icons/logos/kotlin-icon";
import svelteIcon from "@iconify-icons/logos/svelte-icon";
import htmlIcon from "@iconify-icons/logos/html-5";
import cssIcon from "@iconify-icons/logos/css-3-official";
import javascriptIcon from "@iconify-icons/logos/javascript";
import typescriptIcon from "@iconify-icons/logos/typescript-icon";
import rustIcon from "@iconify-icons/logos/rust";
import pythonIcon from "@iconify-icons/logos/python";
import goIcon from "@iconify-icons/logos/go";
import phpIcon from "@iconify-icons/logos/php";
import rubyIcon from "@iconify-icons/logos/ruby";
import swiftIcon from "@iconify-icons/logos/swift";
import dockerIcon from "@iconify-icons/logos/docker-icon";
import jsonIcon from "@iconify-icons/logos/json";
import yamlIcon from "@iconify-icons/logos/yaml";
import markdownIcon from "@iconify-icons/logos/markdown";
import vueIcon from "@iconify-icons/logos/vue";
import astroIcon from "@iconify-icons/logos/astro-icon";
import bashIcon from "@iconify-icons/logos/bash-icon";
import tomlIcon from "@iconify-icons/logos/toml";
import scalaIcon from "@iconify-icons/logos/scala";
import luaIcon from "@iconify-icons/logos/lua";
import perlIcon from "@iconify-icons/logos/perl";
import erlangIcon from "@iconify-icons/logos/erlang";
import haskellIcon from "@iconify-icons/logos/haskell-icon";
import clojureIcon from "@iconify-icons/logos/clojure";
import zigIcon from "@iconify-icons/logos/zig";
import solidityIcon from "@iconify-icons/logos/solidity";
import juliaIcon from "@iconify-icons/logos/julia";
import webassemblyIcon from "@iconify-icons/logos/webassembly";
import graphqlIcon from "@iconify-icons/logos/graphql";
import prismaIcon from "@iconify-icons/logos/prisma";
import gradleIcon from "@iconify-icons/logos/gradle";
import rIcon from "@iconify-icons/logos/r-lang";
import fsharpIcon from "@iconify-icons/logos/fsharp";
import ocamlIcon from "@iconify-icons/logos/ocaml";
import crystalIcon from "@iconify-icons/logos/crystal";
import fortranIcon from "@iconify-icons/logos/fortran";
import terraformIcon from "@iconify-icons/logos/terraform";
import npmIcon from "@iconify-icons/logos/npm-icon";
import yarnIcon from "@iconify-icons/logos/yarn";
import pnpmIcon from "@iconify-icons/logos/pnpm";
import bunIcon from "@iconify-icons/logos/bun";
import denoIcon from "@iconify-icons/logos/deno";
import viteIcon from "@iconify-icons/logos/vitejs";
import webpackIcon from "@iconify-icons/logos/webpack";
import eslintIcon from "@iconify-icons/logos/eslint";
import prettierIcon from "@iconify-icons/logos/prettier";
import tailwindIcon from "@iconify-icons/logos/tailwindcss";

// Exact paths from Icons - Maintained by Yusif Aliyev (MIT).
const CSHARP_PATH = "m11.5 15.97.41 2.44c-.26.14-.68.27-1.24.39-.57.13-1.24.2-2.01.2-2.21-.04-3.87-.7-4.98-1.96C2.56 15.77 2 14.16 2 12.21c.05-2.31.72-4.08 2-5.32C5.32 5.64 6.96 5 8.94 5c.75 0 1.4.07 1.94.19s.94.25 1.2.4l-.58 2.49-1.06-.34c-.4-.1-.86-.15-1.39-.15-1.16-.01-2.12.36-2.87 1.1-.76.73-1.15 1.85-1.18 3.34 0 1.36.37 2.42 1.08 3.2.71.77 1.71 1.17 2.99 1.18l1.33-.12c.43-.08.79-.19 1.1-.32M13.89 19l.61-4H13l.34-2h1.5l.32-2h-1.5L14 9h1.5l.61-4h2l-.61 4h1l.61-4h2l-.61 4H22l-.34 2h-1.5l-.32 2h1.5L21 15h-1.5l-.61 4h-2l.61-4h-1l-.61 4h-2m2.95-6h1l.32-2h-1l-.32 2z";
const JAVA_PATH = "M2 21h18v-2H2M20 8h-2V5h2m0-2H4v10a4 4 0 0 0 4 4h6a4 4 0 0 0 4-4v-3h2a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2z";

const extensionIcons: Record<string, IconifyIcon> = {
  dart: dartIcon, c: cIcon, h: cIcon, cc: cppIcon, cpp: cppIcon, cxx: cppIcon, hpp: cppIcon, hxx: cppIcon,
  kt: kotlinIcon, kts: kotlinIcon, svelte: svelteIcon, html: htmlIcon, htm: htmlIcon,
  css: cssIcon, scss: cssIcon, sass: cssIcon, less: cssIcon,
  js: javascriptIcon, jsx: javascriptIcon, mjs: javascriptIcon, cjs: javascriptIcon,
  ts: typescriptIcon, tsx: typescriptIcon, mts: typescriptIcon, cts: typescriptIcon,
  rs: rustIcon, py: pythonIcon, pyw: pythonIcon, pyi: pythonIcon, go: goIcon, php: phpIcon,
  rb: rubyIcon, swift: swiftIcon, json: jsonIcon, jsonc: jsonIcon, yaml: yamlIcon, yml: yamlIcon,
  md: markdownIcon, mdx: markdownIcon, vue: vueIcon, astro: astroIcon, sh: bashIcon, bash: bashIcon,
  toml: tomlIcon, scala: scalaIcon, sc: scalaIcon, lua: luaIcon, pl: perlIcon, pm: perlIcon,
  erl: erlangIcon, hrl: erlangIcon, hs: haskellIcon, lhs: haskellIcon,
  clj: clojureIcon, cljs: clojureIcon, cljc: clojureIcon, edn: clojureIcon,
  zig: zigIcon, sol: solidityIcon, jl: juliaIcon, wasm: webassemblyIcon, wat: webassemblyIcon,
  graphql: graphqlIcon, gql: graphqlIcon, prisma: prismaIcon, gradle: gradleIcon,
  r: rIcon, rmd: rIcon, fs: fsharpIcon, fsx: fsharpIcon, fsi: fsharpIcon,
  ml: ocamlIcon, mli: ocamlIcon, cr: crystalIcon, f: fortranIcon, f90: fortranIcon, f95: fortranIcon,
  tf: terraformIcon, tfvars: terraformIcon, "vite-config": viteIcon,
  "webpack-config": webpackIcon, "eslint-config": eslintIcon,
  "prettier-config": prettierIcon, "tailwind-config": tailwindIcon,
};

const filenameIcons: Record<string, IconifyIcon> = {
  "package.json": npmIcon, "package-lock.json": npmIcon, "npm-shrinkwrap.json": npmIcon,
  "yarn.lock": yarnIcon, "pnpm-lock.yaml": pnpmIcon, "bun.lock": bunIcon, "bun.lockb": bunIcon,
  "deno.json": denoIcon, "deno.jsonc": denoIcon, "cargo.toml": rustIcon, "cargo.lock": rustIcon,
  "pubspec.yaml": dartIcon, "pubspec.lock": dartIcon, "build.gradle": gradleIcon,
  "settings.gradle": gradleIcon, "chart.yaml": yamlIcon,
};

// Lume's own glyphs for files with no brand of their own: 20x20 outlines drawn like the plain page icon.
const GLYPHS = {
  flask: "M8 3h4M9 3v5.2L4.6 15.6A1.4 1.4 0 0 0 5.8 17.7h8.4a1.4 1.4 0 0 0 1.2-2.1L11 8.2V3M6.6 12.4h6.8",
  sliders: "M4 6h7M14 6h2M4 14h2M9 14h7M11 4v4M6 12v4",
  lock: "M5.5 9h9v7.5h-9zM7.5 9V6.5a2.5 2.5 0 0 1 5 0V9M10 12v1.6",
  key: "M8 6.5a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4ZM10.4 11 16.5 17M13.6 14l1.8-1.8",
  table: "M3.5 5h13v10h-13zM3.5 9h13M3.5 12h13M8.5 5v10",
  database: "M4 5.5c0-1.1 2.7-2 6-2s6 .9 6 2-2.7 2-6 2-6-.9-6-2ZM4 5.5v9c0 1.1 2.7 2 6 2s6-.9 6-2v-9M4 10c0 1.1 2.7 2 6 2s6-.9 6-2",
  text: "M4.5 5h11M4.5 8.5h11M4.5 12h11M4.5 15.5h6.5",
  terminal: "M3.5 4.5h13v11h-13zM6.5 8.5l2.5 2-2.5 2M10.5 12.5h3.2",
  markup: "M7.5 6 3.5 10l4 4M12.5 6l4 4-4 4",
  git: "M6 4.2a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2ZM6 12.6a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2ZM14 6.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2ZM6 7.4v5.2M14 9.6c0 2.6-3.4 2.6-6.4 3.2",
  archive: "M3.5 4.5h13v3h-13zM4.5 7.5v8h11v-8M8 11h4",
  diff: "M6 3.5v13M14 3.5v13M3.5 7h5M11.5 13h5",
  image: "M3.5 4.5h13v11h-13zM3.5 13l4-4 3.5 3.5 2-2 3.5 3.5M12.5 8.2h.01",
  braces: "M8 4.5c-1.8 0-2 1-2 2.2v1.4c0 1-.7 1.9-2 1.9 1.3 0 2 .9 2 1.9v1.4c0 1.2.2 2.2 2 2.2M12 4.5c1.8 0 2 1 2 2.2v1.4c0 1 .7 1.9 2 1.9-1.3 0-2 .9-2 1.9v1.4c0 1.2-.2 2.2-2 2.2",
} as const;
type Glyph = keyof typeof GLYPHS;

// These win over a language logo: .mjs and .toml are scripts and settings more than "JavaScript" and "TOML".
const extensionGlyphs: Record<string, Glyph> = {
  mjs: "flask", ipynb: "flask",
  toml: "sliders", ini: "sliders", cfg: "sliders", conf: "sliders", properties: "sliders", editorconfig: "sliders",
  lock: "lock",
  env: "key", pem: "key", key: "key", crt: "key", cer: "key",
  csv: "table", tsv: "table", xls: "table", xlsx: "table",
  sql: "database", db: "database", sqlite: "database", sqlite3: "database",
  txt: "text", log: "text", rtf: "text",
  bat: "terminal", cmd: "terminal", ps1: "terminal", psm1: "terminal", fish: "terminal", zsh: "terminal",
  xml: "markup", xsd: "markup", xsl: "markup", xslt: "markup", plist: "markup",
  gitignore: "git", gitattributes: "git", gitmodules: "git", gitkeep: "git",
  zip: "archive", tar: "archive", gz: "archive", tgz: "archive", bz2: "archive", xz: "archive", rar: "archive", "7z": "archive",
  patch: "diff", diff: "diff",
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image", bmp: "image", ico: "image", svg: "image", avif: "image",
  proto: "braces", jsonl: "braces", ndjson: "braces", json5: "braces",
};

function fileType(value: string): string {
  const name = value.replaceAll("\\", "/").split("/").pop()?.toLowerCase() ?? "";
  if (name === "dockerfile" || name === "containerfile") return "docker";
  if (name === "pom.xml") return "java";
  if (filenameIcons[name]) return name;
  if (name === ".env" || name.startsWith(".env.")) return "env";
  if (name.startsWith("vite.config.")) return "vite-config";
  if (name.startsWith("webpack.config.")) return "webpack-config";
  if (name.startsWith("eslint.config.") || name.startsWith(".eslintrc")) return "eslint-config";
  if (name.startsWith("prettier.config.") || name.startsWith(".prettierrc")) return "prettier-config";
  if (name.startsWith("tailwind.config.")) return "tailwind-config";
  const extension = name.includes(".") ? name.split(".").pop() ?? "" : "";
  if (extension === "cs" || extension === "csx") return "cs";
  if (["java", "class", "jar"].includes(extension)) return "java";
  return extension;
}

function isMonochromeIcon(icon: IconifyIcon | null): boolean {
  if (!icon) return false;
  if (/(?:linear|radial)Gradient|url\(/i.test(icon.body)) return false;
  const explicitColors = [...icon.body.matchAll(/(?:fill|stroke)=["']([^"']+)["']/gi)]
    .map((match) => match[1].trim().toLowerCase())
    .filter((color) => !["none", "transparent", "currentcolor"].includes(color));
  return new Set(explicitColors).size <= 1;
}

export function fileTypeIconForPath(path: string): { icon: IconifyIcon | null; maintainedPath: string | null; glyph: string | null; sizeOffset: number; monochrome: boolean } {
  const type = fileType(path);
  const glyph = extensionGlyphs[type] ? GLYPHS[extensionGlyphs[type]] : null;
  const icon = glyph ? null : type === "docker" ? dockerIcon : filenameIcons[type] ?? extensionIcons[type] ?? null;
  return {
    icon,
    maintainedPath: type === "cs" ? CSHARP_PATH : type === "java" ? JAVA_PATH : null,
    glyph,
    sizeOffset: type === "go" || type === "php" ? 2 : 0,
    monochrome: isMonochromeIcon(icon),
  };
}

/** Trusted local icon data rendered into the safe Markdown output. */
export function renderFileTypeIconHtml(path: string): string {
  const { icon, maintainedPath, glyph, sizeOffset, monochrome } = fileTypeIconForPath(path);
  if (glyph) return `<svg class="inline-file-icon" viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="${glyph}"/></svg>`;
  if (maintainedPath) return `<svg class="inline-file-icon maintained" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="${maintainedPath}"/></svg>`;
  if (icon) {
    const left = icon.left ?? 0;
    const top = icon.top ?? 0;
    const width = icon.width ?? 16;
    const height = icon.height ?? 16;
    return `<svg class="inline-file-icon brand ${monochrome ? "monochrome" : "multicolor"}${sizeOffset ? " large" : ""}" viewBox="${left} ${top} ${width} ${height}" aria-hidden="true" focusable="false">${icon.body}</svg>`;
  }
  return `<svg class="inline-file-icon" viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="M4 3h8l4 4v10H4Z"/><path d="M12 3v4h4"/></svg>`;
}
