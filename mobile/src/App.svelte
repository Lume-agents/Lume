<script lang="ts">
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { appearanceAttributes } from "$lib/appearance";
  import type { Preferences } from "$lib/domain";
  import { plugin } from "./native";
  import { MobileClient } from "./client.svelte";
  import EntryScreen from "./EntryScreen.svelte";
  import AgentsScreen from "./AgentsScreen.svelte";
  import ChatScreen from "./ChatScreen.svelte";
  import WorkflowsScreen from "./WorkflowsScreen.svelte";
  import SettingsScreen from "./SettingsScreen.svelte";
  import MobileNav from "./MobileNav.svelte";
  import Banner from "./Banner.svelte";

  const client = new MobileClient();
  type Tab = "agents" | "workflows" | "settings";

  let tab = $state<Tab>("agents");
  let chatId = $state<string | null>(null);
  let systemDark = $state(typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)").matches : true);

  const dark = $derived(client.appearance.mode === "system" ? systemDark : client.appearance.mode === "dark");
  // The desktop's appearance code, fed with the phone's choices.
  const look = $derived(appearanceAttributes({
    appearanceTheme: client.appearance.theme,
    accentColor: client.appearance.accentColor || undefined,
    accentOpacity: 100,
    darkBase: client.appearance.darkBase,
    lightBase: client.appearance.lightBase,
    uiFont: client.appearance.uiFont,
    codeFont: client.appearance.codeFont,
  } as unknown as Preferences));
  const chatSession = $derived(chatId ? client.sessions.find((session) => session.id === chatId) ?? null : null);

  function openChat(id: string) {
    chatId = id;
    history.pushState({ lumeChat: id }, "");
  }

  function closeChat() {
    if (history.state?.lumeChat) history.back();
    else chatId = null;
  }

  $effect(() => {
    if (client.requestedView === "settings") {
      chatId = null;
      tab = "settings";
      client.requestedView = null;
    }
  });

  // The status bar and the page behind the app follow the canvas color.
  $effect(() => {
    void look;
    void dark;
    queueMicrotask(() => {
      const root = document.querySelector<HTMLElement>(".mobile-app");
      if (!root) return;
      const color = getComputedStyle(root).backgroundColor;
      document.querySelector('meta[name="theme-color"]')?.setAttribute("content", color);
      document.body.style.setProperty("--mobile-page", color);
    });
  });

  onMount(() => {
    void client.start();
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const followScheme = (event: MediaQueryListEvent) => (systemDark = event.matches);
    scheme.addEventListener("change", followScheme);
    const popstate = () => { if (!history.state?.lumeChat) chatId = null; };
    window.addEventListener("popstate", popstate);
    // Android's back button closes the chat first, then leaves the app as usual.
    const app = plugin<any>("App");
    const back = app?.addListener?.("backButton", () => {
      if (chatId) closeChat();
      else if (tab !== "agents") tab = "agents";
      else void app.exitApp?.();
    });
    return () => {
      scheme.removeEventListener("change", followScheme);
      window.removeEventListener("popstate", popstate);
      void Promise.resolve(back).then((listener) => listener?.remove?.());
    };
  });
</script>

<div
  class:dark
  class="mobile-app text-{client.appearance.textSize}"
  data-appearance={look.theme}
  style={look.baseCss}
  style:--lume-accent={look.accentCss}
  style:--lume-accent-strong={look.accentCss}
  lang={client.language}
>
  {#if client.phase === "app"}
    <div class:behind={Boolean(chatId)} class="tabs">
      {#if tab === "agents"}
        <AgentsScreen {client} onOpenChat={openChat} />
      {:else if tab === "workflows"}
        <WorkflowsScreen {client} onOpenChat={openChat} />
      {:else}
        <SettingsScreen {client} />
      {/if}
      <MobileNav {client} {tab} onSelect={(next) => (tab = next)} />
    </div>
    {#if chatId}
      <div class="chat-layer" transition:fly={{ x: 40, duration: 220, easing: cubicOut, opacity: 0 }}>
        <ChatScreen {client} session={chatSession} onBack={closeChat} />
      </div>
    {/if}
  {:else}
    <EntryScreen {client} />
  {/if}
  <Banner {client} />
</div>

<style>
  .mobile-app { position: fixed; inset: 0; overflow: hidden; }
  .tabs { position: absolute; inset: 0; display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) auto; transition: transform 240ms cubic-bezier(.16, 1, .3, 1), filter 240ms ease; }
  .tabs.behind { transform: translateX(-12%); filter: brightness(.82); pointer-events: none; }
  .chat-layer { position: absolute; z-index: 5; inset: 0; background: var(--workspace-bg); box-shadow: -18px 0 40px rgba(0, 0, 0, .28); }
</style>
