<script lang="ts">
  import { createThreadAvatarConfig } from "$lib/threadAvatar";

  let {
    seed,
    label = "Agent thread",
    size = 36,
  }: {
    seed: string;
    label?: string;
    size?: number;
  } = $props();

  const avatar = $derived(createThreadAvatarConfig(seed));
</script>

<svg
  class="thread-avatar"
  width={size}
  height={size}
  viewBox="0 0 64 64"
  role="img"
  aria-label={label}
  focusable="false"
>
  <title>{label}</title>
  <rect x="1" y="1" width="62" height="62" rx="17" fill={avatar.background} />
  <ellipse cx="32" cy="51" rx="18" ry="5" fill={avatar.deep} opacity="0.15" />

  <g transform={`translate(${avatar.offsetX} ${avatar.offsetY}) rotate(${avatar.tilt} 32 33)`}>
    {#if avatar.body === "cube"}
      <path d="M16 23 23 16h27l-7 7Z" fill={avatar.light} />
      <path d="m43 23 7-7v27l-7 7Z" fill={avatar.shade} />
      <rect x="14" y="23" width="29" height="27" rx="7" fill={avatar.base} />
    {:else if avatar.body === "capsule"}
      <path d="M20 18h23c6 0 10 5 10 11v11c0 5-4 9-9 9H20Z" fill={avatar.shade} />
      <rect x="12" y="18" width="34" height="31" rx="16" fill={avatar.base} />
      <path d="M19 20c6-4 17-4 23 0-8 1-15 3-21 7-3 2-6-4-2-7Z" fill={avatar.light} opacity="0.8" />
    {:else if avatar.body === "gem"}
      <path d="m32 14 18 10-4 22-14 7-17-8-3-21Z" fill={avatar.base} />
      <path d="m32 14 18 10-12 5-26-5Z" fill={avatar.light} />
      <path d="m38 29 12-5-4 22-14 7Z" fill={avatar.shade} />
    {:else if avatar.body === "arch"}
      <path d="M16 31c0-10 7-17 17-17h4c9 0 16 7 16 16v18H16Z" fill={avatar.shade} />
      <path d="M11 31c0-9 7-16 16-16h4c9 0 16 7 16 16v17H11Z" fill={avatar.base} />
      <path d="M16 26c4-9 17-12 26-4-8-2-16 0-22 6-3 3-6 2-4-2Z" fill={avatar.light} opacity="0.82" />
    {:else if avatar.body === "sphere"}
      <circle cx="33" cy="33" r="19" fill={avatar.shade} />
      <circle cx="29" cy="31" r="18" fill={avatar.base} />
      <path d="M17 25c5-10 18-13 27-5-10-2-17 1-24 8-2 2-5 0-3-3Z" fill={avatar.light} opacity="0.88" />
    {:else}
      <path d="M18 17h25l7 7v23H18Z" fill={avatar.shade} />
      <rect x="11" y="24" width="34" height="25" rx="7" fill={avatar.base} />
      <path d="M18 17h25l-6 8H11Z" fill={avatar.light} />
      <rect x="18" y="14" width="24" height="8" rx="4" fill={avatar.base} />
    {/if}

    {#each avatar.eyes as eye}
      <ellipse
        cx={eye.cx}
        cy={eye.cy}
        rx={eye.rx}
        ry={eye.ry}
        fill="#101817"
        transform={`rotate(${eye.rotate} ${eye.cx} ${eye.cy})`}
      />
    {/each}
  </g>
</svg>

<style>
  .thread-avatar {
    display: block;
    flex: 0 0 auto;
    overflow: visible;
    border-radius: 27%;
  }
</style>
