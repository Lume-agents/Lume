import { build } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";

const landingDirectory = path.dirname(fileURLToPath(import.meta.url));
const siteUrl = process.env.LANDING_SITE_URL;
const canonical = siteUrl ? new URL(siteUrl.endsWith("/") ? siteUrl : siteUrl + "/") : null;
if (canonical && !["https:", "http:"].includes(canonical.protocol)) {
  throw new Error("LANDING_SITE_URL must be an http or https URL.");
}

await build({
  configFile: false,
  root: landingDirectory,
  base: "./",
  publicDir: path.join(landingDirectory, "public"),
  plugins: canonical ? [{
    name: "landing-canonical",
    transformIndexHtml(html) {
      return {
        html: html.replace('content="./social.png"', 'content="' + new URL("social.png", canonical).href + '"'),
        tags: [
          { tag: "link", attrs: { rel: "canonical", href: canonical.href }, injectTo: "head" },
          { tag: "meta", attrs: { property: "og:url", content: canonical.href }, injectTo: "head" },
        ],
      };
    },
  }] : [],
  build: {
    outDir: path.join(landingDirectory, "../landing-dist"),
    emptyOutDir: true,
    assetsInlineLimit: 0,
  },
});
