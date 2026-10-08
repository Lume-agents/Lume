<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { renderSafeMarkdown } from "$lib/markdown.js";
  import type { Language } from "$lib/i18n";

  let { text, language }: { text: string; language: Language } = $props();
  let linkError = $state(false);
  const content = $derived(renderSafeMarkdown(text));

  function externalLinks(node: HTMLElement) {
    const follow = (event: MouseEvent) => {
      if (!(event.target instanceof Element) || !("__TAURI_INTERNALS__" in window)) return;
      const link = event.target.closest<HTMLAnchorElement>("a[href]");
      if (!link || !node.contains(link) || !/^(https?:|mailto:)/i.test(link.href)) return;
      event.preventDefault();
      linkError = false;
      void openUrl(link.href).catch(() => { linkError = true; });
    };
    node.addEventListener("click", follow);
    return { destroy: () => node.removeEventListener("click", follow) };
  }
</script>

<div class="orb-response" use:externalLinks>{@html content}</div>
{#if linkError}
  <p class="link-error" role="status">{language === "pt-BR" ? "Não foi possível abrir o link. Tente novamente." : "Could not open the link. Try again."}</p>
{/if}

<style>
  .orb-response { min-width: 0; color: inherit; font-size: 12px; line-height: 1.65; overflow-wrap: anywhere; }
  .orb-response :global(> :first-child) { margin-top: 0; }
  .orb-response :global(> :last-child) { margin-bottom: 0; }
  .orb-response :global(p) { margin: 0 0 10px; }
  .orb-response :global(h1), .orb-response :global(h2), .orb-response :global(h3), .orb-response :global(h4), .orb-response :global(h5), .orb-response :global(h6) { margin: 16px 0 7px; color: inherit; font-size: 13px; font-weight: 700; line-height: 1.4; }
  .orb-response :global(ul), .orb-response :global(ol) { margin: 7px 0 12px; padding-left: 19px; }
  .orb-response :global(li) { margin: 4px 0; }
  .orb-response :global(a) { color: var(--lume-accent-strong, #397b5c); text-decoration: underline; text-underline-offset: 3px; }
  .orb-response :global(code) { padding: 2px 4px; border-radius: 4px; background: color-mix(in srgb, currentColor 7%, transparent); font-family: var(--lume-font-code, "SFMono-Regular", Consolas, monospace); font-size: .92em; }
  .orb-response :global(pre) { max-width: 100%; margin: 10px 0; padding: 10px; overflow-x: auto; border: 1px solid color-mix(in srgb, currentColor 14%, transparent); border-radius: 8px; background: color-mix(in srgb, currentColor 4%, transparent); }
  .orb-response :global(pre code) { padding: 0; background: transparent; white-space: pre; }
  .orb-response :global(blockquote) { margin: 10px 0; padding: 0 0 0 11px; border-left: 1px solid currentColor; }
  .orb-response :global(.markdown-table-wrap) { max-width: 100%; overflow-x: auto; }
  .orb-response :global(table) { width: 100%; border-collapse: collapse; font-size: 11px; }
  .orb-response :global(th), .orb-response :global(td) { padding: 7px; border: 1px solid color-mix(in srgb, currentColor 15%, transparent); text-align: left; }
  .orb-response :global(hr) { margin: 14px 0; border: 0; border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent); }
  .link-error { margin: 8px 0 0; font-size: 11px; }
</style>
