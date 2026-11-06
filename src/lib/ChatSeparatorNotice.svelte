<script lang="ts">
  import LumeIcon from "$lib/LumeIcon.svelte";

  /** Something that happened to the conversation itself: a badge centered between two lines. */
  let { text, icon, alert = false, detail }: { text: string; icon: "stop" | "compact" | "unplug"; alert?: boolean; detail?: string } = $props();
</script>

<div class="chat-separator" class:alert role="status" title={detail}>
  <p><LumeIcon name={icon} size={11} />{text}</p>
</div>

<style>
  .chat-separator { width: 100%; min-width: 0; margin: 6px 0; display: flex; align-items: center; gap: 10px; }
  .chat-separator::before, .chat-separator::after { content: ""; flex: 1 1 0; min-width: 12px; height: 1px; background: var(--workspace-line); }
  p { margin: 0; padding: 3px 9px 3px 7px; min-width: 0; display: flex; align-items: center; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 999px; color: var(--workspace-muted); background: var(--workspace-subtle); font-size: var(--workspace-chat-tiny-size, 12px); font-weight: 680; }
  p :global(.lume-icon) { flex: none; }
  .alert p :global(.lume-icon) { color: #d85c64; }
</style>
