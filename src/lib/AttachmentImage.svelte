<script lang="ts">
  import ImageLightbox from "$lib/ImageLightbox.svelte";
  import { isImageAttachmentPath } from "$lib/imageAttachments";
  import type { Language } from "$lib/i18n";
  import { readLocalImageDataUrl } from "$lib/lume";

  /**
   * A photo attached to a prompt. Its thumbnail comes with the prompt, or is read from the file when
   * the prompt was recovered from the agent's own conversation; clicking opens the whole photo.
   */
  let { attachment, language = "en", width, height, fit = "cover", radius = 10 } = $props<{
    attachment: { name: string; previewDataUrl?: string; path?: string | null };
    language?: Language;
    width?: number | string;
    height?: number | string;
    fit?: "cover" | "contain";
    radius?: number;
  }>();
  let loaded = $state("");
  let open = $state(false);
  const thumbnail = $derived(attachment.previewDataUrl || loaded);
  const size = (value?: number | string) => (typeof value === "number" ? `${value}px` : value);

  $effect(() => {
    const path = attachment.path;
    if (attachment.previewDataUrl || !path || !isImageAttachmentPath(path)) return;
    let cancelled = false;
    void readLocalImageDataUrl(path).then((value) => { if (!cancelled) loaded = value; }).catch(() => undefined);
    return () => { cancelled = true; };
  });

  const loadFull = $derived(attachment.path && isImageAttachmentPath(attachment.path) ? () => readLocalImageDataUrl(attachment.path as string) : undefined);
</script>

{#if thumbnail}
  <button
    type="button"
    class="attachment-image"
    style:width={size(width)}
    style:height={size(height)}
    style:border-radius={`${radius}px`}
    title={language === "pt-BR" ? "Ver a imagem inteira" : "View the whole image"}
    aria-label={language === "pt-BR" ? `Ver ${attachment.name}` : `View ${attachment.name}`}
    onclick={() => (open = true)}
  >
    <img src={thumbnail} alt={attachment.name} style:object-fit={fit} style:border-radius={`${radius}px`} />
  </button>
  {#if open}
    <ImageLightbox src={thumbnail} alt={attachment.name} load={loadFull} {language} onclose={() => (open = false)} />
  {/if}
{:else}
  <span class="attachment-file" title={attachment.name}>{attachment.name}</span>
{/if}

<style>
  .attachment-image { padding: 0; display: block; overflow: hidden; border: 0; background: none; cursor: zoom-in; }
  .attachment-image:focus-visible { outline: 2px solid var(--lume-accent-strong, #4d9d76); outline-offset: 2px; }
  img { width: 100%; height: 100%; display: block; }
  .attachment-file { max-width: 190px; padding: 6px 8px; overflow: hidden; border: 1px solid currentColor; border-radius: 7px; font-size: 9px; opacity: 0.8; text-overflow: ellipsis; white-space: nowrap; }
</style>
