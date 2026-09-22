import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { relative } from "node:path";

const host = process.env.TAURI_DEV_HOST;

/** @returns {import('vite').Plugin} */
function recoverSvelteStyleCache() {
  /** @type {import('vite').ViteDevServer | undefined} */
  let server;
  const recovering = new Set();

  return {
    name: "lume-recover-svelte-style-cache",
    enforce: "post",
    apply: "serve",
    configureServer(viteServer) {
      server = viteServer;
    },
    async load(id) {
      const [filename, query] = id.split("?", 2);
      const params = new URLSearchParams(query);
      if (
        !server ||
        !filename.endsWith(".svelte") ||
        !params.has("svelte") ||
        params.get("type") !== "style" ||
        recovering.has(id)
      ) {
        return;
      }

      // A style request can race the component transform after a WebView reload.
      // Populate Svelte's CSS cache before Vite falls back to the raw .svelte file.
      recovering.add(id);
      try {
        const url = `/${relative(process.cwd(), filename).replaceAll("\\", "/")}`;
        await server.transformRequest(url);
        return await server.pluginContainer.load(id);
      } finally {
        recovering.delete(id);
      }
    },
  };
}

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit(), recoverSvelteStyleCache()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
