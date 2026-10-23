import { defineConfig } from "vite";
import { svelte, vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// Lume Mobile: the same Svelte modules as the Workspace, built into mobile-pwa/ with fixed file names.
// The desktop gateway embeds these files (src-tauri/src/mobile_server.rs), GitHub Pages publishes them
// and Capacitor packages them, so every build must produce index.html, app.js and styles.css.
const root = fileURLToPath(new URL("./mobile", import.meta.url));

export default defineConfig({
  root,
  base: "./",
  publicDir: fileURLToPath(new URL("./mobile/public", import.meta.url)),
  plugins: [svelte({ configFile: false, preprocess: vitePreprocess() })],
  resolve: {
    alias: { $lib: fileURLToPath(new URL("./src/lib", import.meta.url)) },
  },
  // Fonts ride inside styles.css: the gateway only serves the fixed files above.
  build: {
    outDir: fileURLToPath(new URL("./mobile-pwa", import.meta.url)),
    emptyOutDir: true,
    assetsInlineLimit: 1024 * 1024,
    cssCodeSplit: false,
    target: "es2022",
    rollupOptions: {
      output: {
        entryFileNames: "app.js",
        assetFileNames: (asset) => (asset.names?.some((name) => name.endsWith(".css")) ? "styles.css" : "[name][extname]"),
        inlineDynamicImports: true,
      },
    },
  },
  server: { port: 1433, strictPort: true },
});
