<script lang="ts">
  import "../app.css";
  import LumeTooltipHost from "$lib/LumeTooltipHost.svelte";

  let { children } = $props();
</script>

{@render children()}
<LumeTooltipHost />
