<!--
  The chat panel of a Telegram chat: the last messages as text, and a box to send a text message.
  All texts come from Telegram. The panel shows them as text, never as HTML.
-->
<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, errorText, TELEGRAM_CHAT_CHANGED, type ChatMessage, type ChatPanel, type TimelineItem } from "./api";
  import { t, type Key } from "./i18n.svelte";

  let { item, accountName, onclose }: { item: TimelineItem; accountName: string; onclose: () => void } = $props();

  // The parent makes a new panel for each chat, so the key of the item does not change.
  const { account, source_id: sourceId } = untrack(() => item.key);

  let panel = $state<ChatPanel | null>(null);
  let error = $state("");
  let text = $state("");
  let sending = $state(false);
  let list = $state<HTMLElement | null>(null);

  async function scrollDown() {
    await Promise.resolve();
    list?.scrollTo({ top: list.scrollHeight });
  }

  async function open() {
    try {
      panel = await api.telegramChatOpen(account, sourceId);
      scrollDown();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function refresh() {
    try {
      const atEnd = !list || list.scrollHeight - list.scrollTop - list.clientHeight < 40;
      panel = await api.telegramChat(account, sourceId);
      if (atEnd) scrollDown();
    } catch {
      // The panel keeps the last messages. The next event tries again.
    }
  }

  $effect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    open();
    listen(TELEGRAM_CHAT_CHANGED, refresh)
      .then((unlisten) => {
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  });

  onDestroy(() => {
    api.telegramChatClose(account).catch(() => {});
  });

  async function send() {
    error = "";
    sending = true;
    try {
      // The answer command sends a plain text message for a Telegram item.
      await api.answer(account, sourceId, text);
      text = "";
      await refresh();
      scrollDown();
    } catch (e) {
      error = errorText(e);
    } finally {
      sending = false;
    }
  }

  function messageText(message: ChatMessage): string {
    if (!message.kind) return message.text;
    const label = `[${t(`tg_kind_${message.kind}` as Key)}]`;
    return message.text ? `${label} ${message.text}` : label;
  }

  function keydown(e: KeyboardEvent) {
    // Enter sends. Shift+Enter starts a new line.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && text.trim() && !sending) {
      e.preventDefault();
      send();
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="panel" role="dialog" aria-modal="true" tabindex="-1" onkeydown={(e) => e.key === "Escape" && onclose()}>
    <header class="row">
      <div class="title">
        <h2>{panel?.title || item.author}</h2>
        <span class="muted small">{accountName}</span>
      </div>
      <span class="spacer"></span>
      <button class="quiet" onclick={onclose} aria-label={t("close")}>×</button>
    </header>

    <div class="messages" bind:this={list}>
      {#if !panel && !error}
        <p class="muted small">{t("tg_chat_loading")}</p>
      {:else if panel && panel.messages.length === 0}
        <p class="muted small">{t("tg_chat_empty")}</p>
      {/if}
      {#each panel?.messages ?? [] as message (message.id)}
        <div class="message" class:out={message.outgoing}>
          <span class="meta small muted">
            {message.outgoing ? t("you") : message.sender}
            · <time>{new Date(message.time).toLocaleString()}</time>
            {#if message.sending === "pending"}· {t("tg_sending_pending")}{/if}
            {#if message.sending === "failed"}<span class="failed">· {t("tg_sending_failed")}</span>{/if}
          </span>
          <span class="body" class:label={message.kind}>{messageText(message)}</span>
        </div>
      {/each}
    </div>

    <footer>
      <p class="muted small">{t("tg_chat_text_only")}</p>
      {#if error}<div class="error">{error}</div>{/if}
      {#if panel && !panel.can_write}
        <p class="muted small">{t("tg_chat_cannot_write")}</p>
      {:else if panel}
        <textarea
          rows="3"
          bind:value={text}
          onkeydown={keydown}
          aria-label={t("tg_message_placeholder")}
          placeholder={t("answer_placeholder_telegram", { name: accountName })}
        ></textarea>
        <div class="row">
          <span class="spacer"></span>
          <button class="primary" disabled={sending || !text.trim()} onclick={send}>
            {sending ? t("sending") : t("send")}
          </button>
        </div>
      {/if}
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.3);
    display: flex;
    justify-content: flex-end;
    z-index: 10;
  }

  .panel {
    width: min(520px, 100vw);
    height: 100%;
    background: var(--surface);
    border-left: 1px solid var(--line);
    display: grid;
    grid-template-rows: auto 1fr auto;
  }

  header {
    padding: 16px 18px;
    border-bottom: 1px solid var(--line);
  }

  .title {
    display: grid;
    min-width: 0;
  }

  .title h2 {
    margin: 0;
    font-size: 17px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .messages {
    overflow-y: auto;
    padding: 14px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .message {
    display: grid;
    gap: 2px;
    max-width: 85%;
    align-self: flex-start;
    padding: 8px 10px;
    border-radius: 10px;
    background: var(--surface-2);
  }

  .message.out {
    align-self: flex-end;
    background: var(--peri-soft);
  }

  .body {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .body.label {
    font-style: italic;
  }

  .failed {
    color: var(--danger);
  }

  footer {
    padding: 12px 18px 16px;
    border-top: 1px solid var(--line);
    display: grid;
    gap: 8px;
  }

  footer p {
    margin: 0;
  }

  textarea {
    width: 100%;
    padding: 8px 10px;
    font: inherit;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 8px;
    resize: vertical;
  }

  textarea:focus-visible {
    outline: none;
    box-shadow: var(--focus);
  }
</style>
