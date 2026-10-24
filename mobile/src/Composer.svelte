<script lang="ts">
  import { fade, fly, slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { HubSession } from "$lib/hubProtocol";
  import { displayText } from "$lib/i18n";
  import LumeIcon from "$lib/LumeIcon.svelte";
  import type { MobileClient } from "./client.svelte";
  import { MAX_IMAGES, clipboardImageFiles, prepareImage, readNativeClipboardImage, type PreparedImage } from "./images";

  let { client, session }: { client: MobileClient; session: HubSession } = $props();

  let textarea = $state<HTMLTextAreaElement | null>(null);
  let fileInput = $state<HTMLInputElement | null>(null);
  let takeover = $state(false);
  let selections = $state<Record<string, string>>({});
  let answering = $state(false);
  let deciding = $state<string | null>(null);

  const id = $derived(session.id);
  const draft = $derived(client.drafts[id] ?? "");
  const attached = $derived(client.attachments[id] ?? []);
  const sending = $derived(Boolean(client.sending[id]));
  const canPrompt = $derived(Boolean(client.canPrompt(session)));
  const canTakeControl = $derived(client.scopes.includes("prompt") && Boolean(session.capabilities?.canTakeControl));
  const canAttach = $derived(canPrompt && Boolean(session.capabilities?.canAttachImages));
  const deliveries = $derived(session.capabilities?.promptDeliveries ?? []);
  const running = $derived(session.status === "running");
  const delivery = $derived(client.deliveries[id] ?? "queue");
  const question = $derived(session.pendingQuestion);
  const permission = $derived(session.pendingPermission);
  const canSend = $derived(!sending && (draft.trim().length > 0 || attached.length > 0) && (!running || deliveries.length > 0 || canTakeControl));
  const placeholder = $derived(question
    ? client.tr("Answer or option number", "Resposta ou número da opção")
    : canTakeControl ? client.tr("Take control and continue", "Assumir e continuar")
    : running ? (delivery === "steer" ? client.tr("Steer this turn", "Direcionar este turno") : client.tr("Next prompt", "Próximo prompt"))
    : client.tr("Message", "Mensagem"));

  function setDraft(value: string) {
    client.drafts = { ...client.drafts, [id]: value };
  }

  function grow() {
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${Math.min(textarea.scrollHeight, 168)}px`;
  }

  function attach(prepared: PreparedImage[]) {
    const room = Math.max(0, MAX_IMAGES - attached.length);
    if (!prepared.length || !room) return;
    client.attachments = { ...client.attachments, [id]: [...attached, ...prepared.slice(0, room)] };
  }

  async function chooseFiles(files: FileList | File[]) {
    try {
      const prepared: PreparedImage[] = [];
      for (const file of [...files].slice(0, MAX_IMAGES - attached.length)) prepared.push(await prepareImage(file));
      attach(prepared);
    } catch (error) {
      client.showBanner((error as Error)?.message || client.tr("Could not attach this image.", "Não foi possível anexar esta imagem."), "error");
    }
  }

  async function paste(event: ClipboardEvent) {
    if (!canAttach || sending) return;
    const files = clipboardImageFiles(event);
    const imageHint = [...(event.clipboardData?.types || [])].some((type) => type === "Files" || type.toLowerCase().startsWith("image/"));
    if (!files.length && !imageHint) return;
    event.preventDefault();
    if (files.length) return void chooseFiles(files);
    const prepared = await readNativeClipboardImage().catch(() => null);
    if (prepared) attach([prepared]);
    else client.showBanner(client.tr("Could not read the image from the clipboard.", "Não foi possível ler a imagem da área de transferência."), "error");
  }

  function removeAttachment(index: number) {
    client.attachments = { ...client.attachments, [id]: attached.filter((_: PreparedImage, position: number) => position !== index) };
  }

  /** An agent question takes the typed text as its answer: a number picks that option. */
  async function answerWithText(text: string) {
    if (!question || question.questions.length !== 1) return false;
    const item = question.questions[0];
    const number = Number.parseInt(text, 10);
    const option = /^\d+$/.test(text) ? item.options[number - 1] : undefined;
    answering = true;
    const ok = await client.answerQuestion(session, [{ questionId: item.id, answers: [option?.label ?? text] }]);
    answering = false;
    if (ok) setDraft("");
    return true;
  }

  async function chooseOption(questionId: string, label: string) {
    selections = { ...selections, [questionId]: label };
    if (question && question.questions.length === 1) await submitAnswers();
  }

  async function submitAnswers() {
    if (!question) return;
    const answers = question.questions.map((item) => ({ questionId: item.id, answers: selections[item.id] ? [selections[item.id]] : [] }));
    if (answers.some((answer) => !answer.answers.length)) {
      client.showBanner(client.tr("Choose one option for each question.", "Escolha uma opção para cada pergunta."), "error");
      return;
    }
    answering = true;
    if (await client.answerQuestion(session, answers)) selections = {};
    answering = false;
  }

  async function decide(action: string) {
    deciding = action;
    await client.resolvePermission(session, action);
    deciding = null;
  }

  async function send(confirmed = false) {
    const text = draft.trim();
    if (question && text && !attached.length && await answerWithText(text)) return;
    const result = await client.sendPrompt(session, confirmed);
    if (result === "takeover") takeover = true;
    else if (result === "sent") {
      takeover = false;
      requestAnimationFrame(grow);
    }
  }

  function permissionLabel(action: string) {
    return ({ allow_once: client.tr("Allow once", "Permitir uma vez"), allow_session: client.tr("Allow for session", "Permitir na sessão"), deny: client.tr("Deny", "Negar") } as Record<string, string>)[action] ?? action;
  }
</script>

<div class="composer">
  {#if permission}
    <section class="permission risk-{permission.risk}" transition:slide={{ duration: 180, easing: cubicOut }}>
      <header><LumeIcon name="warning" size={15} /><strong>{displayText(client.language, permission.summary)}</strong></header>
      {#if permission.resource}<code>{permission.resource}</code>{/if}
      {#if client.scopes.includes("approve") && session.capabilities?.canApprove}
        <div class="actions">
          {#each session.permissionProfile?.availableActions ?? ["allow_once", "deny"] as action (action)}
            <button class:allow={action === "allow_once"} class:deny={action === "deny"} type="button" disabled={Boolean(deciding)} onclick={() => void decide(action)}>
              {#if deciding === action}<i class="spinner" aria-hidden="true"></i>{/if}{permissionLabel(action)}
            </button>
          {/each}
        </div>
      {:else}
        <p class="note">{client.tr("Approvals are off for this phone. Turn them on in Lume on your computer.", "As aprovações estão desligadas para este celular. Ative no Lume do computador.")}</p>
      {/if}
    </section>
  {/if}

  {#if question}
    <section class="question" transition:slide={{ duration: 180, easing: cubicOut }}>
      {#each question.questions as item (item.id)}
        <div class="question-item">
          <small>{displayText(client.language, item.header)}</small>
          <strong>{displayText(client.language, item.question)}</strong>
          {#if item.options.length}
            <div class="options">
              {#each item.options as option, index (option.label)}
                <button class:selected={selections[item.id] === option.label} type="button" disabled={answering} onclick={() => void chooseOption(item.id, option.label)}>
                  <b>{index + 1}</b>
                  <span>{displayText(client.language, option.label)}{#if option.description}<small>{displayText(client.language, option.description)}</small>{/if}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
      {#if question.questions.length > 1}
        <button class="answer" type="button" disabled={answering} onclick={() => void submitAnswers()}>{client.tr("Answer", "Responder")}</button>
      {/if}
    </section>
  {/if}

  {#if canPrompt}
    {#if running && deliveries.includes("steer") && deliveries.includes("queue")}
      <div class="delivery" role="group" aria-label={client.tr("While the agent works", "Enquanto o agente trabalha")}>
        <button class:on={delivery === "queue"} type="button" onclick={() => (client.deliveries = { ...client.deliveries, [id]: "queue" })}>{client.tr("Queue", "Fila")}</button>
        <button class:on={delivery === "steer"} type="button" onclick={() => (client.deliveries = { ...client.deliveries, [id]: "steer" })}>{client.tr("Steer now", "Direcionar agora")}</button>
      </div>
    {/if}
    {#if attached.length}
      <div class="attached">
        {#each attached as image, index (index)}
          <span class="attached-image"><img src={image.previewDataUrl} alt={image.name} /><button type="button" aria-label={client.tr(`Remove ${image.name}`, `Remover ${image.name}`)} onclick={() => removeAttachment(index)}><LumeIcon name="close" size={12} /></button></span>
        {/each}
      </div>
    {/if}
    <form class="field" class:sending onsubmit={(event) => { event.preventDefault(); void send(); }}>
      {#if canAttach}
        <button class="attach" type="button" aria-label={client.tr("Attach image", "Anexar imagem")} disabled={sending || attached.length >= MAX_IMAGES} onclick={() => fileInput?.click()}><LumeIcon name="plus" size={20} /></button>
        <input bind:this={fileInput} class="file-input" type="file" accept="image/*" multiple onchange={(event) => { void chooseFiles(event.currentTarget.files ?? []); event.currentTarget.value = ""; }} />
      {/if}
      <textarea
        bind:this={textarea}
        value={draft}
        rows="1"
        maxlength="16384"
        aria-label={placeholder}
        {placeholder}
        disabled={sending}
        oninput={(event) => { setDraft(event.currentTarget.value); grow(); }}
        onpaste={(event) => void paste(event)}
      ></textarea>
      <button class="send" type="submit" disabled={!canSend} aria-label={sending ? client.tr("Sending prompt", "Enviando prompt") : client.tr("Send prompt", "Enviar prompt")}>
        {#if sending}<i class="spinner" aria-hidden="true"></i>{:else}<LumeIcon name="send" size={18} />{/if}
      </button>
    </form>
  {:else if !permission && !question}
    <p class="unavailable">{client.scopes.includes("prompt")
      ? client.tr("This session cannot receive messages from the phone.", "Esta sessão não recebe mensagens pelo celular.")
      : client.tr("Prompts are off for this phone. Turn them on in Lume on your computer.", "Os prompts estão desligados para este celular. Ative no Lume do computador.")}</p>
  {/if}
</div>

{#if takeover}
  <div class="dialog-backdrop" transition:fade={{ duration: 150 }} onclick={(event) => { if (event.target === event.currentTarget) takeover = false; }} role="presentation">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="takeover-title" transition:fly={{ y: 24, duration: 200, easing: cubicOut }}>
      <small>{client.tr("External CLI", "CLI externa")}</small>
      <strong id="takeover-title">{client.tr("Take control from Lume?", "Assumir o controle pelo Lume?")}</strong>
      <p>{running
        ? client.tr("This agent is still working. Lume will stop the external CLI, reopen the same thread in a managed terminal on your computer, and then send this message.", "Este agente ainda está trabalhando. O Lume vai parar a CLI externa, reabrir a mesma conversa num terminal gerenciado no computador e então enviar esta mensagem.")
        : client.tr("Lume will close the external CLI, reopen the same thread in a managed terminal on your computer, and then send this message.", "O Lume vai fechar a CLI externa, reabrir a mesma conversa num terminal gerenciado no computador e então enviar esta mensagem.")}</p>
      <div class="dialog-actions">
        <button type="button" onclick={() => (takeover = false)}>{client.tr("Cancel", "Cancelar")}</button>
        <button class="primary" type="button" disabled={sending} onclick={() => void send(true)}>{sending ? client.tr("Taking control…", "Assumindo…") : client.tr("Take control & send", "Assumir e enviar")}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .composer { padding: 8px 10px calc(var(--safe-bottom) + 10px); display: grid; gap: 8px; border-top: 1px solid var(--workspace-line); background: color-mix(in srgb, var(--workspace-sidebar) 92%, transparent); backdrop-filter: blur(14px); }
  .permission { padding: 12px; display: grid; gap: 9px; border: 1px solid color-mix(in srgb, #d0903f 40%, var(--workspace-line)); border-radius: 16px; background: color-mix(in srgb, #d0903f 8%, var(--workspace-raised)); }
  .permission.risk-high { border-color: color-mix(in srgb, #d0645c 50%, var(--workspace-line)); background: color-mix(in srgb, #d0645c 8%, var(--workspace-raised)); }
  .permission header { display: flex; align-items: flex-start; gap: 8px; color: #d39a4c; }
  .permission header strong { color: var(--workspace-strong); font-size: 14.5px; line-height: 1.35; }
  .permission code { max-height: 120px; padding: 8px 10px; overflow: auto; border-radius: 10px; color: var(--workspace-strong); background: var(--workspace-code); font: 12.5px/1.45 var(--lume-font-code, monospace); white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .actions button { min-height: 42px; padding: 6px 12px; display: inline-flex; align-items: center; justify-content: center; gap: 7px; flex: 1 1 30%; border: 1px solid var(--workspace-line); border-radius: 12px; color: var(--workspace-strong); background: var(--workspace-raised); font-size: 13.5px; font-weight: 700; }
  .actions button.allow { border-color: transparent; color: #fff; background: var(--workspace-accent); }
  .actions button.deny { color: #d76e66; }
  .note { margin: 0; color: var(--workspace-muted); font-size: 12.5px; }
  .question { max-height: 46vh; padding: 12px; display: grid; gap: 12px; overflow-y: auto; border: 1px solid color-mix(in srgb, var(--workspace-accent) 35%, var(--workspace-line)); border-radius: 16px; background: var(--workspace-raised); }
  .question-item { display: grid; gap: 6px; }
  .question-item > small { color: var(--workspace-accent); font-size: 11.5px; font-weight: 750; letter-spacing: .03em; text-transform: uppercase; }
  .question-item > strong { color: var(--workspace-strong); font-size: 14.5px; line-height: 1.4; }
  .options { display: grid; gap: 6px; }
  .options button { padding: 10px; display: flex; align-items: flex-start; gap: 10px; border: 1px solid var(--workspace-line); border-radius: 12px; color: var(--workspace-strong); background: var(--workspace-bg); text-align: left; font-size: 14px; }
  .options button.selected { border-color: var(--workspace-accent); background: var(--workspace-accent-soft); }
  .options b { width: 24px; height: 24px; display: grid; place-items: center; flex: 0 0 auto; border-radius: 8px; color: var(--workspace-accent); background: var(--workspace-accent-soft); font-size: 12.5px; }
  .options span { display: grid; gap: 2px; }
  .options small { color: var(--workspace-muted); font-size: 12.5px; }
  .answer { height: 42px; border: 0; border-radius: 12px; color: #fff; background: var(--workspace-accent); font-weight: 720; }
  .delivery { display: inline-flex; justify-self: start; gap: 2px; padding: 3px; border-radius: 11px; background: var(--workspace-subtle); }
  .delivery button { height: 30px; padding: 0 12px; border: 0; border-radius: 8px; color: var(--workspace-muted); background: transparent; font-size: 12.5px; font-weight: 680; }
  .delivery button.on { color: var(--workspace-strong); background: var(--workspace-raised); box-shadow: 0 1px 3px rgba(0, 0, 0, .18); }
  .attached { display: flex; gap: 8px; overflow-x: auto; }
  .attached-image { position: relative; width: 62px; height: 62px; flex: 0 0 auto; overflow: hidden; border: 1px solid var(--workspace-line); border-radius: 12px; }
  .attached-image img { width: 100%; height: 100%; object-fit: cover; }
  .attached-image button { position: absolute; top: 3px; right: 3px; width: 22px; height: 22px; padding: 0; display: grid; place-items: center; border: 0; border-radius: 50%; color: #fff; background: rgba(0, 0, 0, .6); }
  .field { min-height: 52px; padding: 5px; display: flex; align-items: flex-end; gap: 6px; border: 1px solid var(--workspace-line); border-radius: 22px; background: var(--workspace-raised); }
  .field:focus-within { border-color: color-mix(in srgb, var(--workspace-accent) 50%, var(--workspace-line)); }
  .attach, .send { width: 40px; height: 40px; display: grid; place-items: center; flex: 0 0 auto; border: 0; border-radius: 50%; }
  .attach { color: var(--workspace-muted); background: var(--workspace-subtle); }
  .send { color: #fff; background: var(--workspace-accent); }
  .send:disabled { opacity: .4; }
  .file-input { display: none; }
  textarea { min-width: 0; min-height: 40px; max-height: 168px; padding: 9px 4px 9px 8px; flex: 1; border: 0; outline: 0; resize: none; color: var(--workspace-strong); background: transparent; font-size: 16px; line-height: 1.4; }
  textarea::placeholder { color: var(--workspace-faint); }
  .unavailable { margin: 4px 6px; color: var(--workspace-muted); font-size: 13px; text-align: center; }
  .spinner { width: 16px; height: 16px; border: 2px solid rgba(255, 255, 255, .35); border-top-color: currentColor; border-radius: 50%; animation: spin 700ms linear infinite; }
  .dialog-backdrop { position: fixed; z-index: 30; inset: 0; padding: 16px; display: grid; align-items: end; background: rgba(4, 9, 6, .55); backdrop-filter: blur(3px); }
  .dialog { padding: 20px 18px calc(var(--safe-bottom) + 16px); display: grid; gap: 8px; border: 1px solid var(--workspace-line); border-radius: 22px; background: var(--workspace-raised); }
  .dialog small { color: var(--workspace-accent); font-size: 11.5px; font-weight: 750; letter-spacing: .04em; text-transform: uppercase; }
  .dialog strong { color: var(--workspace-strong); font-size: 18px; }
  .dialog p { margin: 0 0 6px; color: var(--workspace-muted); font-size: 14px; line-height: 1.5; }
  .dialog-actions { display: flex; gap: 8px; }
  .dialog-actions button { height: 46px; flex: 1; border: 1px solid var(--workspace-line); border-radius: 13px; color: var(--workspace-strong); background: transparent; font-weight: 700; }
  .dialog-actions .primary { border-color: transparent; color: #fff; background: var(--workspace-accent); }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
