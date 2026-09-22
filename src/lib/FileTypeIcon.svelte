<script lang="ts">
  import Icon from "@iconify/svelte";
  import { fileTypeIconForPath } from "$lib/fileTypeIcons";

  let { path, size = 14 } = $props<{ path: string; size?: number }>();
  const presentation = $derived(fileTypeIconForPath(path));
  const displaySize = $derived(size + presentation.sizeOffset);
</script>

<span class:brand={Boolean(presentation.icon)} class:monochrome={presentation.monochrome} class:multicolor={Boolean(presentation.icon) && !presentation.monochrome} class:maintained={Boolean(presentation.maintainedPath)} class="file-type-icon" aria-hidden="true" style:width={`${size}px`} style:height={`${size}px`}>
  {#if presentation.maintainedPath}
    <svg class="maintained-icon" viewBox="0 0 24 24" style:width={`${displaySize}px`} style:height={`${displaySize}px`}><path d={presentation.maintainedPath} /></svg>
  {:else if presentation.icon}
    <Icon icon={presentation.icon} width={displaySize} height={displaySize} />
  {:else}
    <svg viewBox="0 0 20 20">
      <path d="M4 3h8l4 4v10H4Z" /><path d="M12 3v4h4" />
    </svg>
  {/if}
</span>

<style>
  .file-type-icon { display: inline-grid; place-items: center; flex: 0 0 auto; color: inherit; opacity: .72; }
  .file-type-icon.brand { opacity: .9; }
  .file-type-icon.brand.monochrome { filter: var(--file-monochrome-filter, grayscale(1) brightness(0) contrast(.88)); opacity: var(--file-monochrome-opacity, .72); }
  .file-type-icon.brand.multicolor { filter: none; opacity: .92; }
  .file-type-icon.brand :global(svg),
  .file-type-icon.brand :global(svg *) { stroke: none !important; stroke-width: 0 !important; }
  .file-type-icon.maintained { color: inherit; opacity: .76; }
  svg { width: 100%; height: 100%; fill: none; stroke: currentColor; stroke-width: 1.35; stroke-linecap: round; stroke-linejoin: round; }
  .maintained-icon { fill: currentColor; stroke: none; }
</style>
