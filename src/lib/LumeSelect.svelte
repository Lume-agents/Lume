<script lang="ts">
  import { onMount } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { fly } from "svelte/transition";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import { copyResolvedColorTokens } from "$lib/floatingTheme";

  export type LumeSelectOption = {
    value: string;
    label: string;
    description?: string;
  };

  let {
    value,
    options,
    ariaLabel,
    onValueChange,
    minWidth = 120,
    menuMinWidth = minWidth,
    variant = "field",
    disabled = false,
  }: {
    value: string;
    options: LumeSelectOption[];
    ariaLabel: string;
    onValueChange: (value: string) => void;
    minWidth?: number;
    menuMinWidth?: number;
    variant?: "field" | "heading";
    disabled?: boolean;
  } = $props();

  let root = $state<HTMLDivElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);
  let menuNode: HTMLDivElement | null = null;
  let open = $state(false);
  let menuLeft = $state(0);
  let menuTop = $state(0);
  let menuWidth = $state(120);
  let menuMaxHeight = $state(220);
  let menuAbove = $state(false);
  let reducedMotion = $state(false);
  let activeIndex = $state(0);
  const selected = $derived(options.find((option) => option.value === value) ?? options[0]);
  $effect(() => { if (disabled) open = false; });

  function syncFloatingTheme() {
    if (!root || !menuNode) return;
    copyResolvedColorTokens(root, menuNode, ["surface", "text", "muted", "line", "accent", "hover", "active"].map((token) => ({ source: `--select-${token}` })));
  }

  function floatMenu(node: HTMLDivElement) {
    menuNode = node;
    document.body.appendChild(node);
    syncFloatingTheme();
    return {
      destroy() {
        if (menuNode === node) menuNode = null;
        node.remove();
      },
    };
  }

  function placeMenu() {
    if (!trigger) return;
    syncFloatingTheme();
    const bounds = trigger.getBoundingClientRect();
    const estimatedHeight = Math.min(230, options.length * 43 + 10);
    const below = window.innerHeight - bounds.bottom - 8;
    const above = bounds.top - 8;
    const placeAbove = below < Math.min(150, estimatedHeight) && above > below;
    menuAbove = placeAbove;
    menuWidth = Math.min(Math.max(bounds.width, menuMinWidth), window.innerWidth - 16);
    menuLeft = Math.max(8, Math.min(window.innerWidth - menuWidth - 8, bounds.right - menuWidth));
    menuMaxHeight = Math.max(92, Math.min(230, placeAbove ? above - 5 : below - 5));
    menuTop = placeAbove
      ? Math.max(8, bounds.top - Math.min(estimatedHeight, menuMaxHeight) - 5)
      : bounds.bottom + 5;
  }

  function toggle() {
    if (disabled) return;
    if (open) { open = false; return; }
    activeIndex = Math.max(0, options.findIndex((option) => option.value === value));
    placeMenu();
    open = true;
  }

  function choose(option: LumeSelectOption) {
    if (disabled) return;
    open = false;
    if (option.value !== value) onValueChange(option.value);
    trigger?.focus();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      open = false;
      trigger?.focus();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) { toggle(); return; }
      if (!options.length) return;
      const direction = event.key === "ArrowDown" ? 1 : -1;
      activeIndex = (activeIndex + direction + options.length) % options.length;
      return;
    }
    if ((event.key === "Enter" || event.key === " ") && open) {
      event.preventDefault();
      if (options[activeIndex]) choose(options[activeIndex]);
    }
  }

  onMount(() => {
    const motionPreference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const syncMotion = () => (reducedMotion = motionPreference.matches);
    syncMotion();
    motionPreference.addEventListener("change", syncMotion);
    const closeOutside = (event: PointerEvent) => {
      if (open && root && !root.contains(event.target as Node) && !menuNode?.contains(event.target as Node)) open = false;
    };
    const reposition = () => open && placeMenu();
    document.addEventListener("pointerdown", closeOutside);
    window.addEventListener("resize", reposition);
    document.addEventListener("scroll", reposition, true);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      window.removeEventListener("resize", reposition);
      document.removeEventListener("scroll", reposition, true);
      motionPreference.removeEventListener("change", syncMotion);
    };
  });
</script>

<div class="lume-select" class:heading={variant === "heading"} bind:this={root} style:min-width="{minWidth}px">
  <button
    bind:this={trigger}
    class:open
    class="lume-select-trigger"
    type="button"
    aria-label={variant === "heading" && selected ? `${ariaLabel}: ${selected.label}` : ariaLabel}
    aria-haspopup="listbox"
    aria-expanded={open}
    {disabled}
    onkeydown={handleKeydown}
    onclick={toggle}
  >
    <span>{selected?.label ?? "—"}</span>
    <span class="select-chevron"><LumeIcon name="chevron-down" size={13} /></span>
  </button>

  {#if open}
    <div
      class="lume-select-menu"
      use:floatMenu
      role="listbox"
      tabindex="-1"
      aria-label={ariaLabel}
      style:left="{menuLeft}px"
      style:top="{menuTop}px"
      style:width="{menuWidth}px"
      style:max-height="{menuMaxHeight}px"
      in:fly={{ y: reducedMotion ? 0 : menuAbove ? 6 : -6, duration: reducedMotion ? 80 : 160, easing: cubicOut }}
      out:fly={{ y: reducedMotion ? 0 : menuAbove ? 6 : -6, duration: reducedMotion ? 70 : 110, easing: cubicOut }}
      onkeydown={handleKeydown}
      onpointerdown={(event) => event.stopPropagation()}
    >
      {#each options as option, index (option.value)}
        <button
          class:active={option.value === value}
          class:focused={index === activeIndex}
          type="button"
          role="option"
          aria-selected={option.value === value}
          onpointerenter={() => (activeIndex = index)}
          onclick={() => choose(option)}
        >
          <span><strong>{option.label}</strong>{#if option.description}<small>{option.description}</small>{/if}</span>
          {#if option.value === value}<LumeIcon name="check" size={13} />{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .lume-select { --select-surface: var(--workspace-raised, var(--dropdown-surface, var(--lume-raised-light))); --select-text: var(--workspace-text, var(--dropdown-text, var(--lume-ink-light))); --select-muted: var(--workspace-muted, var(--dropdown-muted, var(--lume-ink-muted-light))); --select-line: var(--workspace-line, var(--dropdown-line, var(--lume-line-light))); --select-accent: var(--workspace-accent, var(--dropdown-accent, var(--lume-accent-strong))); --select-hover: var(--workspace-subtle, var(--dropdown-hover, var(--lume-subtle-light))); --select-active: var(--workspace-accent-soft, var(--lume-accent-soft-light)); position: relative; min-width: 0; flex: 0 1 auto; }
  :global(.overlay-shell.dark) .lume-select,
  :global(.workspace.dark) .lume-select,
  :global(.terminal-window.dark) .lume-select,
  :global(.bridge-window.dark) .lume-select { --select-surface: var(--lume-raised-dark); --select-text: var(--lume-ink-dark); --select-muted: var(--lume-ink-muted-dark); --select-line: var(--lume-line-dark); --select-accent: var(--lume-accent); --select-hover: var(--lume-subtle-dark); --select-active: var(--lume-accent-soft-dark); }
  .lume-select-trigger { width: 100%; min-height: 30px; padding: 0 8px 0 10px; display: flex; align-items: center; gap: 7px; border: 1px solid var(--select-line); border-radius: 9px; color: var(--select-text); background: var(--select-surface); cursor: pointer; text-align: left; transition: border-color 140ms ease, background 140ms ease; }
  .lume-select-trigger:disabled { opacity: .55; cursor: not-allowed; }
  .lume-select-trigger:hover,
  .lume-select-trigger.open { border-color: color-mix(in srgb, var(--select-accent) 35%, var(--select-line)); background: color-mix(in srgb, var(--select-accent) 5%, var(--select-surface)); }
  .lume-select-trigger > span:first-child { min-width: 0; flex: 1; overflow: hidden; font: 700 9px Inter, sans-serif; text-overflow: ellipsis; white-space: nowrap; }
  .heading .lume-select-trigger { min-height: 22px; padding: 2px 4px; border: 0; border-radius: 4px; color: var(--workspace-strong, var(--select-text)); background: transparent; }
  .heading .lume-select-trigger > span:first-child { flex: 0 1 auto; font: inherit; font-size: 11px; font-weight: 700; letter-spacing: -.015em; }
  .heading .lume-select-trigger:hover,
  .heading .lume-select-trigger.open { background: var(--select-hover); }
  .select-chevron { min-width: 13px; display: grid; place-items: center; flex: 0 0 auto; color: var(--select-muted); transition: transform 160ms cubic-bezier(.16, 1, .3, 1); }
  .lume-select-trigger.open > .select-chevron { transform: rotate(180deg); }
  .lume-select-trigger:focus-visible { outline: 2px solid color-mix(in srgb, var(--select-accent) 45%, transparent); outline-offset: 2px; }
  .lume-select-menu { position: fixed; z-index: 260; padding: 5px; display: grid; gap: 2px; overflow-y: auto; isolation: isolate; contain: paint; overscroll-behavior: contain; scrollbar-width: thin; scrollbar-color: var(--select-line) transparent; border: 1px solid var(--select-line); border-radius: 11px; color: var(--select-text); background: var(--select-surface); background-clip: padding-box; box-shadow: 0 16px 38px rgba(12, 20, 16, .19); }
  .lume-select-menu > button { width: 100%; min-height: 34px; padding: 5px 7px; display: flex; align-items: center; gap: 6px; border: 0; border-radius: 8px; color: var(--select-text); background: transparent; cursor: pointer; text-align: left; }
  .lume-select-menu > button:hover,
  .lume-select-menu > button.focused { background: var(--select-hover); }
  .lume-select-menu > button.active { color: var(--select-accent); background: var(--select-active); }
  .lume-select-menu > button > span { min-width: 0; flex: 1; display: grid; gap: 1px; }
  .lume-select-menu strong { overflow: hidden; font: 750 8px Inter, sans-serif; text-overflow: ellipsis; white-space: nowrap; }
  .lume-select-menu small { overflow: hidden; color: var(--select-muted); font: 7px Inter, sans-serif; text-overflow: ellipsis; white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) { .select-chevron { transition: none; } }
</style>
