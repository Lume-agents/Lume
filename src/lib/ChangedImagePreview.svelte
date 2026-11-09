<script lang="ts">
  import ImageLightbox from "$lib/ImageLightbox.svelte";
  import type { FileChangeKind } from "$lib/fileChanges";
  import type { Language } from "$lib/i18n";
  import { readChangedImageDataUrl } from "$lib/lume";

  /** The image an agent created or changed, as it is on disk now; clicking it opens it whole. */
  let { path, name, kind, language = "en" } = $props<{ path: string; name: string; kind: FileChangeKind; language?: Language }>();
  const tr = (english: string, portuguese: string) => (language === "pt-BR" ? portuguese : english);

  let src = $state("");
  let failure = $state("");
  let open = $state(false);
  const label = $derived(kind === "created" ? tr("Image created", "Imagem criada") : kind === "deleted" ? tr("Image removed", "Imagem removida") : tr("Image edited", "Imagem editada"));

  $effect(() => {
    if (kind === "deleted") return;
    let cancelled = false;
    src = "";
    failure = "";
    void readChangedImageDataUrl(path)
      .then((value) => { if (!cancelled) src = value; })
      .catch((error) => { if (!cancelled) failure = String(error).replace(/^Error:\s*/, ""); });
    return () => { cancelled = true; };
  });
</script>

<div class="changed-image" data-kind={kind}>
  <span class="change-badge {kind}">{label}</span>
  {#if kind === "deleted"}
    <p>{tr("The file is no longer there.", "O arquivo não existe mais.")}</p>
  {:else if src}
    <button type="button" class="image-frame" title={tr("View the whole image", "Ver a imagem inteira")} aria-label={tr(`View ${name}`, `Ver ${name}`)} onclick={() => (open = true)}>
      <img {src} alt={name} draggable="false" />
    </button>
    {#if kind === "edited"}<small>{tr("Shows the file as it is now.", "Mostra o arquivo como está agora.")}</small>{/if}
  {:else if failure}
    <p role="alert">{failure}</p>
  {:else}
    <div class="image-frame loading" aria-busy="true"><i></i></div>
  {/if}
</div>
{#if open && src}
  <ImageLightbox {src} alt={name} {language} onclose={() => (open = false)} />
{/if}

<style>
  .changed-image { display: grid; gap: 8px; justify-items: start; padding: 10px; }
  .changed-image p { margin: 0; color: var(--workspace-muted); font-size: 12px; line-height: 1.5; }
  .changed-image small { color: var(--workspace-muted); font-size: 10px; }
  .change-badge { padding: 2px 8px; border-radius: 999px; font-size: 10px; font-weight: 700; letter-spacing: .02em; color: var(--workspace-muted); background: var(--workspace-subtle); }
  .change-badge.created { color: #3f9b69; background: color-mix(in srgb, #3f9b69 14%, transparent); }
  .change-badge.edited { color: #b98a2c; background: color-mix(in srgb, #d0a142 16%, transparent); }
  .change-badge.deleted { color: #c0554f; background: color-mix(in srgb, #c0554f 14%, transparent); }
  .image-frame { max-width: 100%; padding: 0; display: block; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 10px; background-color: var(--workspace-subtle); background-image: conic-gradient(color-mix(in srgb, currentColor 7%, transparent) 25%, transparent 0 50%, color-mix(in srgb, currentColor 7%, transparent) 0 75%, transparent 0); background-size: 14px 14px; cursor: zoom-in; }
  .image-frame:focus-visible { outline: 2px solid var(--workspace-accent); outline-offset: 2px; }
  .image-frame img { max-width: 100%; max-height: 230px; display: block; object-fit: contain; }
  .image-frame.loading { width: 160px; height: 90px; display: grid; place-items: center; cursor: default; }
  .image-frame.loading i { width: 18px; height: 18px; border: 2px solid var(--workspace-line); border-top-color: var(--workspace-accent); border-radius: 50%; animation: image-spin 700ms linear infinite; }
  @keyframes image-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .image-frame.loading i { animation: none; } }
</style>
