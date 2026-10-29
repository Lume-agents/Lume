<script lang="ts">
  // An author's GitHub picture, or their initials on a color of their own while there is none.
  let { name, email = "", url = "", size = 20 }: { name: string; email?: string; url?: string; size?: number } = $props();
  let failed = $state(false);
  $effect(() => { void url; failed = false; });
  const initials = $derived(name.trim().split(/\s+/).slice(0, 2).map((part) => part[0] ?? "").join("").toUpperCase() || "?");
  const hue = $derived([...(email || name)].reduce((sum, char) => (sum * 31 + char.charCodeAt(0)) % 360, 7));
</script>

<span class="avatar" style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.42)}px" style:--hue={hue} title={email ? `${name} <${email}>` : name}>
  {#if url && !failed}<img src={url} alt="" width={size} height={size} loading="lazy" referrerpolicy="no-referrer" onerror={() => (failed = true)} />{:else}{initials}{/if}
</span>

<style>
  .avatar { flex: 0 0 auto; display: inline-grid; place-items: center; overflow: hidden; border-radius: 50%; color: hsl(var(--hue) 45% 88%); background: hsl(var(--hue) 32% 34%); font-weight: 700; line-height: 1; user-select: none; }
  img { width: 100%; height: 100%; object-fit: cover; }
</style>
