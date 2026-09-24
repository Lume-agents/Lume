import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.dirname(fileURLToPath(import.meta.url));
const server = await createServer({
  configFile: false,
  root,
  base: "./",
  publicDir: path.join(root, "public"),
  server: {
    host: "127.0.0.1",
    port: 4174,
    strictPort: true,
    fs: { deny: ["**/.git/**", "**/.env*", "**/*.md"] },
  },
});
await server.listen();
server.printUrls();
