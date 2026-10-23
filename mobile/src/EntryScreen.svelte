<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import LumeMascot from "$lib/LumeMascot.svelte";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { MobileClient } from "./client.svelte";

  let { client } = $props<{ client: MobileClient }>();

  // svelte-ignore state_referenced_locally
  let manualGateway = $state(client.apiBase || ""); // The form starts from the last gateway, then is the person's.
  let manualCode = $state("");
  const apkUrl = "https://github.com/Lume-agents/Lume/releases/latest/download/Lume-Mobile.apk";
  const androidBrowser = typeof navigator !== "undefined" && /Android/i.test(navigator.userAgent);
</script>

<section class="entry" in:fade={{ duration: 180 }}>
  {#if client.phase === "starting" || client.phase === "loading"}
    <div class="loading">
      <LumeMascot status="running" size={64} />
      <p>{client.phase === "loading" ? client.tr("Connecting to Lume…", "Conectando ao Lume…") : client.tr("Starting…", "Abrindo…")}</p>
    </div>
  {:else if client.phase === "pair"}
    <div class="hero">
      <LumeMascot status="idle" awake size={64} />
      <h1>{client.tr("Connect to Lume", "Conectar ao Lume")}</h1>
      <p>{client.tr("This one-time code links only this device. Allow local network access when your phone asks.", "Este código de uso único conecta só este aparelho. Permita o acesso à rede local quando o celular pedir.")}</p>
    </div>
    <form class="card" onsubmit={(event) => { event.preventDefault(); void client.pair(); }}>
      <label><span>{client.tr("Device name", "Nome do aparelho")}</span><input bind:value={client.deviceName} maxlength="80" autocomplete="name" required /></label>
      <button class="primary" type="submit" disabled={client.pairing}>{client.pairing ? client.tr("Connecting…", "Conectando…") : client.tr("Connect securely", "Conectar com segurança")}</button>
      {#if client.pairMessage}<p class="message" aria-live="polite">{client.pairMessage}</p>{/if}
    </form>
    <p class="privacy"><LumeIcon name="shield" size={14} />{client.tr("Encrypted locally. No certificate or Lume cloud account required.", "Criptografado localmente. Sem certificado nem conta na nuvem do Lume.")}</p>
  {:else}
    <div class="hero">
      <LumeMascot status={client.phase === "failed" ? "failed" : "idle"} size={64} />
      <h1>{client.tr("Pair with your computer", "Parear com o computador")}</h1>
      <p>{client.tr("Open Lume → Settings → Mobile access on your computer, then scan the QR code or enter the connection details below.", "No computador, abra Lume → Ajustes → Acesso pelo celular e escaneie o QR ou preencha os dados abaixo.")}</p>
    </div>
    {#if client.canScan}
      <button class="scan" type="button" onclick={() => void client.scanPairingCode()}>
        <span class="scan-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M7 12h10" /></svg></span>
        <span><strong>{client.tr("Scan QR code", "Escanear QR")}</strong><small>{client.tr("Use the camera to pair automatically", "Use a câmera para parear automaticamente")}</small></span>
      </button>
    {/if}
    <form class="card" onsubmit={(event) => { event.preventDefault(); void client.pair({ gateway: manualGateway, code: manualCode }); }}>
      <label><span>{client.tr("Local gateway", "Gateway local")}</span><input bind:value={manualGateway} inputmode="url" placeholder="http://192.168.1.10:43124" required /></label>
      <label><span>{client.tr("One-time code", "Código de uso único")}</span><input bind:value={manualCode} autocomplete="one-time-code" required /></label>
      <label><span>{client.tr("Device name", "Nome do aparelho")}</span><input bind:value={client.deviceName} maxlength="80" required /></label>
      <button class="primary" type="submit" disabled={client.pairing}>{client.pairing ? client.tr("Connecting…", "Conectando…") : client.tr("Connect locally", "Conectar localmente")}</button>
      <p class="hint"><span>1</span>{client.tr("Keep the phone and computer on the same Wi-Fi network.", "Deixe o celular e o computador na mesma rede Wi-Fi.")}</p>
      {#if client.pairMessage}<p class="message" aria-live="polite">{client.pairMessage}</p>{/if}
    </form>
    {#if androidBrowser && client.platform === "web"}
      <a class="download" href={apkUrl} rel="noopener">{client.tr("Download the Android app", "Baixar o app para Android")}</a>
    {/if}
  {/if}
</section>

{#if client.showInstallPrompt}
  <div class="install-backdrop" transition:fade={{ duration: 160 }}>
    <div class="install" role="dialog" aria-modal="true" aria-labelledby="install-title" transition:fly={{ y: 24, duration: 220, easing: cubicOut }}>
      <button class="close" type="button" aria-label={client.tr("Close", "Fechar")} onclick={() => client.dismissInstallPrompt()}><LumeIcon name="close" size={16} /></button>
      <LumeMascot status="idle" awake size={48} />
      <small>{client.tr("Lume for Android", "Lume para Android")}</small>
      <strong id="install-title">{client.tr("Continue in the mobile app", "Continue no app do celular")}</strong>
      <p>{client.tr("Open the installed app, or download it before pairing.", "Abra o app instalado ou baixe-o antes de parear.")}</p>
      <a class="primary" href={client.appLink}>{client.tr("Open Lume Mobile", "Abrir o Lume Mobile")}</a>
      <a class="download" href={apkUrl} rel="noopener">{client.tr("Download the app", "Baixar o app")}</a>
      <button class="quiet" type="button" onclick={() => client.dismissInstallPrompt()}>{client.tr("Continue in browser", "Continuar no navegador")}</button>
    </div>
  </div>
{/if}

<style>
  .entry { position: absolute; inset: 0; padding: calc(var(--safe-top) + 34px) 20px calc(var(--safe-bottom) + 28px); display: flex; flex-direction: column; gap: 16px; overflow-y: auto; }
  .loading { margin: auto; display: grid; justify-items: center; gap: 14px; color: var(--workspace-muted); font-size: 14px; }
  .hero { display: grid; justify-items: center; gap: 10px; text-align: center; }
  h1 { margin: 6px 0 0; color: var(--workspace-strong); font-size: 23px; font-weight: 750; letter-spacing: -.02em; }
  .hero p { margin: 0; max-width: 340px; color: var(--workspace-muted); font-size: 14.5px; line-height: 1.5; }
  .card { padding: 16px; display: grid; gap: 13px; border: 1px solid var(--workspace-line); border-radius: 18px; background: var(--workspace-raised); }
  label { display: grid; gap: 6px; }
  label span { color: var(--workspace-muted); font-size: 12.5px; font-weight: 650; }
  input { height: 46px; padding: 0 13px; border: 1px solid var(--workspace-line); border-radius: 12px; color: var(--workspace-strong); background: var(--workspace-bg); font-size: 16px; outline: 0; }
  input:focus { border-color: color-mix(in srgb, var(--workspace-accent) 60%, var(--workspace-line)); }
  .primary { height: 48px; display: grid; place-items: center; border: 0; border-radius: 13px; color: #fff; background: var(--workspace-accent); font-size: 15px; font-weight: 720; text-decoration: none; }
  .primary:disabled { opacity: .6; }
  .message { margin: 0; color: #d7867f; font-size: 13.5px; line-height: 1.45; }
  .hint { margin: 0; display: flex; align-items: center; gap: 8px; color: var(--workspace-muted); font-size: 12.5px; }
  .hint span { width: 20px; height: 20px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 50%; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-weight: 750; }
  .privacy { margin: 0; display: flex; align-items: center; justify-content: center; gap: 7px; color: var(--workspace-faint); font-size: 12.5px; text-align: center; }
  .scan { padding: 14px; display: flex; align-items: center; gap: 14px; border: 1px solid color-mix(in srgb, var(--workspace-accent) 40%, var(--workspace-line)); border-radius: 18px; color: var(--workspace-strong); background: var(--workspace-accent-soft); text-align: left; }
  .scan-icon { width: 44px; height: 44px; display: grid; place-items: center; border-radius: 13px; color: #fff; background: var(--workspace-accent); }
  .scan-icon svg { width: 24px; height: 24px; fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; }
  .scan span { display: grid; gap: 3px; }
  .scan strong { font-size: 15.5px; }
  .scan small { color: var(--workspace-muted); font-size: 12.5px; }
  .download { align-self: center; color: var(--workspace-accent); font-size: 14px; font-weight: 650; text-underline-offset: 3px; }
  .install-backdrop { position: absolute; z-index: 30; inset: 0; padding: 20px; display: grid; align-items: end; background: rgba(4, 9, 6, .6); backdrop-filter: blur(4px); }
  .install { position: relative; padding: 24px 20px calc(var(--safe-bottom) + 18px); display: grid; justify-items: center; gap: 8px; border: 1px solid var(--workspace-line); border-radius: 24px; background: var(--workspace-raised); text-align: center; }
  .install small { color: var(--workspace-accent); font-size: 12px; font-weight: 750; letter-spacing: .04em; text-transform: uppercase; }
  .install strong { color: var(--workspace-strong); font-size: 19px; }
  .install p { margin: 0 0 6px; color: var(--workspace-muted); font-size: 14px; }
  .install .primary { width: 100%; }
  .close { position: absolute; top: 12px; right: 12px; width: 34px; height: 34px; display: grid; place-items: center; border: 0; border-radius: 10px; color: var(--workspace-muted); background: transparent; }
  .quiet { margin-top: 4px; padding: 8px; border: 0; color: var(--workspace-muted); background: transparent; font-size: 14px; }
</style>
