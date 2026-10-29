<script lang="ts">
  import LumeIcon, { type LumeIconName } from "$lib/LumeIcon.svelte";
  import { statusLabel, type FileStatus } from "$lib/fileStatus";

  // A file's state as a small colored mark: green new, amber modified, red deleted, blue renamed.
  let { status, language = "en" }: { status: FileStatus; language?: "en" | "pt-BR" } = $props();
  const icons: Record<FileStatus, LumeIconName> = { added: "plus", modified: "rename", deleted: "minus", renamed: "arrow-right", conflict: "warning", staged: "check" };
  const label = $derived(statusLabel(status, language === "pt-BR"));
</script>

<span class="mark {status}" title={label} role="img" aria-label={label}><LumeIcon name={icons[status]} size={11} strokeWidth={2.2} /></span>

<style>
  .mark { width: 17px; height: 17px; flex: 0 0 17px; display: inline-grid; place-items: center; border-radius: 5px; }
  .added { color: #4daa77; background: rgba(77, 170, 119, .15); }
  .modified { color: #d6a441; background: rgba(214, 164, 65, .15); }
  .deleted { color: #d0625c; background: rgba(208, 98, 92, .15); }
  .renamed { color: #4d99cc; background: rgba(77, 153, 204, .15); }
  .conflict { color: #e0703c; background: rgba(224, 112, 60, .17); }
  .staged { color: #3fb8af; background: rgba(63, 184, 175, .15); }
</style>
