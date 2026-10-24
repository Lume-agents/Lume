<script lang="ts">
  import { appearanceThemes, darkBases, lightBases } from "$lib/appearance";
  import { codeFonts, uiFonts } from "$lib/fonts";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { MobileAppearance, MobileClient } from "./client.svelte";

  let { client }: { client: MobileClient } = $props();

  const pt = $derived(client.language === "pt-BR");
  const scopeNames = $derived([
    ["prompt", client.tr("Prompts", "Prompts")],
    ["approve", client.tr("Approvals", "Aprovações")],
    ["terminate", client.tr("Stop agents", "Parar agentes")],
  ] as const);

  function set<K extends keyof MobileAppearance>(key: K, value: MobileAppearance[K]) {
    client.setAppearance({ [key]: value } as Partial<MobileAppearance>);
  }

  async function disconnect() {
    if (confirm(client.tr("Disconnect this phone from Lume?", "Desconectar este celular do Lume?"))) await client.disconnect();
  }
</script>

<section class="screen">
  <header class="top"><h1>{client.tr("Settings", "Ajustes")}</h1></header>
  <div class="scroll">
    <h2>{client.tr("Appearance", "Aparência")}</h2>
    <div class="group">
      <div class="block">
        <span class="label">{client.tr("Theme", "Tema")}</span>
        <div class="themes">
          {#each appearanceThemes as theme (theme.value)}
            <button class:on={client.appearance.theme === theme.value} type="button" aria-pressed={client.appearance.theme === theme.value} onclick={() => set("theme", theme.value)}>
              <span class="swatch" style:--surface={theme.darkSurface} style:--light={theme.lightSurface} style:--accent={theme.accent}><i></i></span>
              <small>{theme.label}</small>
            </button>
          {/each}
        </div>
      </div>
      <div class="row">
        <span class="label">{client.tr("Mode", "Modo")}</span>
        <div class="segmented">
          {#each [["system", client.tr("System", "Sistema")], ["dark", client.tr("Dark", "Escuro")], ["light", client.tr("Light", "Claro")]] as [value, label] (value)}
            <button class:on={client.appearance.mode === value} type="button" onclick={() => set("mode", value as MobileAppearance["mode"])}>{label}</button>
          {/each}
        </div>
      </div>
      <label class="row">
        <span class="label">{client.tr("Dark surfaces", "Superfícies escuras")}</span>
        <select value={client.appearance.darkBase} onchange={(event) => set("darkBase", event.currentTarget.value as MobileAppearance["darkBase"])}>
          {#each darkBases.filter((base) => base.value !== "custom") as base (base.value)}<option value={base.value}>{pt ? base.labelPt : base.label}</option>{/each}
        </select>
      </label>
      <label class="row">
        <span class="label">{client.tr("Light surfaces", "Superfícies claras")}</span>
        <select value={client.appearance.lightBase} onchange={(event) => set("lightBase", event.currentTarget.value as MobileAppearance["lightBase"])}>
          {#each lightBases.filter((base) => base.value !== "custom") as base (base.value)}<option value={base.value}>{pt ? base.labelPt : base.label}</option>{/each}
        </select>
      </label>
      <label class="row">
        <span class="label">{client.tr("Interface font", "Fonte da interface")}</span>
        <select value={client.appearance.uiFont || "default"} onchange={(event) => set("uiFont", event.currentTarget.value)}>
          {#each uiFonts as font (font.value)}<option value={font.value}>{pt ? font.labelPt : font.label}</option>{/each}
        </select>
      </label>
      <label class="row">
        <span class="label">{client.tr("Code font", "Fonte do código")}</span>
        <select value={client.appearance.codeFont || "default"} onchange={(event) => set("codeFont", event.currentTarget.value)}>
          {#each codeFonts as font (font.value)}<option value={font.value}>{pt ? font.labelPt : font.label}</option>{/each}
        </select>
      </label>
      <div class="row">
        <span class="label">{client.tr("Text size", "Tamanho do texto")}</span>
        <div class="segmented">
          {#each [["small", "A−"], ["default", "A"], ["large", "A+"]] as [value, label] (value)}
            <button class:on={client.appearance.textSize === value} type="button" onclick={() => set("textSize", value as MobileAppearance["textSize"])}>{label}</button>
          {/each}
        </div>
      </div>
      <div class="row">
        <span class="label">{client.tr("Language", "Idioma")}</span>
        <div class="segmented">
          <button class:on={client.language === "en"} type="button" onclick={() => client.setLanguage("en")}>English</button>
          <button class:on={client.language === "pt-BR"} type="button" onclick={() => client.setLanguage("pt-BR")}>Português</button>
        </div>
      </div>
    </div>

    <h2>{client.tr("Notifications and security", "Notificações e segurança")}</h2>
    <div class="group">
      <button class="row toggle" type="button" onclick={() => void client.toggleNotifications()}>
        <span><strong>{client.tr("Notifications", "Notificações")}</strong><small>{client.tr("Tasks, errors, approvals and questions", "Tarefas, erros, aprovações e perguntas")}</small></span>
        <i class:on={client.notifications} class="switch"></i>
      </button>
      {#if client.nativeBackground}
        <button class="row toggle" type="button" onclick={() => void client.toggleBackground()}>
          <span><strong>{client.tr("Background monitoring", "Monitorar em segundo plano")}</strong><small>{client.tr("Keep watching with the app closed", "Continuar acompanhando com o app fechado")}</small></span>
          <i class:on={client.background} class="switch"></i>
        </button>
      {/if}
      <button class="row toggle" type="button" onclick={() => void client.toggleBiometric()}>
        <span><strong>{client.tr("Device verification", "Verificação do aparelho")}</strong><small>{client.tr("Fingerprint or face before remote actions", "Digital ou rosto antes de ações remotas")}</small></span>
        <i class:on={client.biometric} class="switch"></i>
      </button>
    </div>

    <h2>{client.tr("This device", "Este aparelho")}</h2>
    <div class="group">
      <div class="row">
        <span><strong>{client.device?.name ?? client.tr("Paired phone", "Celular pareado")}</strong><small>{client.apiBase}</small></span>
      </div>
      <div class="block">
        <span class="label">{client.tr("Allowed from this phone", "Permitido neste celular")}</span>
        <div class="scopes">
          {#each scopeNames as [scope, label] (scope)}
            <span class:on={client.scopes.includes(scope)} class="scope"><LumeIcon name={client.scopes.includes(scope) ? "check" : "close"} size={12} />{label}</span>
          {/each}
        </div>
        <small class="hint">{client.tr("Change these in Lume on your computer, under Mobile access.", "Mude isso no Lume do computador, em Acesso pelo celular.")}</small>
      </div>
      <button class="row danger" type="button" onclick={() => void disconnect()}><strong>{client.tr("Disconnect this phone", "Desconectar este celular")}</strong></button>
    </div>

    <h2>{client.tr("App", "App")}</h2>
    <div class="group">
      <div class="row">
        <span><strong>Lume Mobile</strong><small>{client.update.version || client.update.detail}</small>{#if client.update.version && client.update.detail}<small>{client.update.detail}</small>{/if}</span>
        {#if !client.update.managed}
          <button class:primary={Boolean(client.update.available)} class="small" type="button" disabled={client.update.busy} onclick={() => void (client.update.available ? client.installUpdate() : client.checkUpdate(true))}>
            {client.update.busy ? client.tr("Working…", "Aguarde…") : client.update.available ? client.tr("Install", "Instalar") : client.tr("Check now", "Verificar")}
          </button>
        {/if}
      </div>
    </div>
  </div>
</section>

<style>
  .screen { min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
  .top { padding: calc(var(--safe-top) + 14px) 18px 6px; }
  h1 { margin: 0; color: var(--workspace-strong); font-size: 24px; font-weight: 760; letter-spacing: -.02em; }
  .scroll { min-height: 0; padding: 4px 14px 28px; overflow-y: auto; }
  h2 { margin: 20px 6px 8px; color: var(--workspace-faint); font-size: 12px; font-weight: 750; letter-spacing: .05em; text-transform: uppercase; }
  .group { overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 18px; background: var(--workspace-raised); }
  .row, .block { width: 100%; min-height: 56px; padding: 12px 14px; display: flex; align-items: center; justify-content: space-between; gap: 12px; border: 0; border-bottom: 1px solid var(--workspace-line); color: var(--workspace-strong); background: transparent; text-align: left; }
  .block { display: grid; justify-content: stretch; }
  .group > :last-child { border-bottom: 0; }
  .row > span { min-width: 0; display: grid; gap: 2px; }
  .row strong { font-size: 15px; font-weight: 650; }
  .row small, .hint { overflow: hidden; color: var(--workspace-muted); font-size: 12.5px; text-overflow: ellipsis; }
  .label { color: var(--workspace-strong); font-size: 15px; font-weight: 650; }
  .themes { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 8px; }
  .themes button { padding: 6px 0; display: grid; justify-items: center; gap: 5px; border: 0; color: var(--workspace-muted); background: transparent; }
  .swatch { position: relative; width: 44px; height: 44px; overflow: hidden; border: 2px solid transparent; border-radius: 14px; background: linear-gradient(135deg, var(--surface) 50%, var(--light) 50%); box-shadow: inset 0 0 0 1px rgba(255, 255, 255, .08); }
  .swatch i { position: absolute; right: 6px; bottom: 6px; width: 14px; height: 14px; border-radius: 50%; background: var(--accent); }
  .themes button.on .swatch { border-color: var(--workspace-accent); }
  .themes button.on small { color: var(--workspace-accent); font-weight: 700; }
  .themes small { font-size: 11.5px; }
  .segmented { display: inline-flex; gap: 2px; padding: 3px; border-radius: 12px; background: var(--workspace-subtle); }
  .segmented button { height: 32px; padding: 0 11px; border: 0; border-radius: 9px; color: var(--workspace-muted); background: transparent; font-size: 13px; font-weight: 680; }
  .segmented button.on { color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: 0 1px 3px rgba(0, 0, 0, .18); }
  select { max-width: 52%; height: 36px; padding: 0 10px; border: 1px solid var(--workspace-line); border-radius: 10px; color: var(--workspace-strong); background: var(--workspace-bg); font-size: 14px; }
  .switch { position: relative; width: 46px; height: 28px; flex: 0 0 auto; border-radius: 999px; background: color-mix(in srgb, var(--workspace-muted) 35%, transparent); transition: background 160ms ease; }
  .switch::after { position: absolute; top: 3px; left: 3px; width: 22px; height: 22px; border-radius: 50%; background: #fff; box-shadow: 0 1px 3px rgba(0, 0, 0, .3); content: ""; transition: transform 180ms cubic-bezier(.2, .8, .2, 1); }
  .switch.on { background: var(--workspace-accent); }
  .switch.on::after { transform: translateX(18px); }
  .scopes { display: flex; flex-wrap: wrap; gap: 6px; }
  .scope { height: 30px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border-radius: 999px; color: var(--workspace-faint); background: var(--workspace-subtle); font-size: 12.5px; font-weight: 650; }
  .scope.on { color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .danger strong { color: #d76e66; }
  .small { height: 36px; padding: 0 14px; flex: 0 0 auto; border: 1px solid var(--workspace-line); border-radius: 11px; color: var(--workspace-strong); background: transparent; font-size: 13.5px; font-weight: 700; }
  .small.primary { border-color: transparent; color: #fff; background: var(--workspace-accent); }
</style>
