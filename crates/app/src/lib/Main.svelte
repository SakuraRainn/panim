<!-- The open app: the accounts on the left, the timeline, the proofs or the settings on the right. -->
<script lang="ts">
  import AddAccount from "./AddAccount.svelte";
  import ChatPanel from "./ChatPanel.svelte";
  import Mark from "./Mark.svelte";
  import ProofsPage from "./ProofsPage.svelte";
  import SettingsPage from "./SettingsPage.svelte";
  import { listen } from "@tauri-apps/api/event";
  import {
    api,
    errorText,
    TIMELINE_CHANGED,
    UNREAD_CHANGED,
    type Account,
    type Status,
    type SyncView,
    type TimelineItem,
    type Unread,
  } from "./api";
  import { t, type Key } from "./i18n.svelte";

  let { status, onchange }: { status: Status; onchange: () => void } = $props();

  let view = $state<"timeline" | "proofs" | "settings">("timeline");
  let accounts = $state<Account[]>([]);
  let items = $state<TimelineItem[]>([]);
  let unreadOnly = $state(false);
  let adding = $state(false);
  let notice = $state("");
  let unread = $state<Record<string, Unread>>({});
  // The state of the joined accounts: the unread count and the result of the last sync.
  let sync = $state<Record<string, SyncView>>({});
  // The timeline shows only this account, or all accounts.
  let accountFilter = $state<number | null>(null);
  let syncing = $state(false);
  // The key of the item that has the open answer box.
  let answering = $state<string | null>(null);
  let answerText = $state("");
  let sending = $state(false);
  let info = $state("");
  // The Telegram chat item that has the open chat panel.
  let chatItem = $state<TimelineItem | null>(null);

  const itemId = (item: TimelineItem) => item.key.account + ":" + item.key.source_id;
  const accountName = (id: number) => accounts.find((a) => a.id === id)?.display_name ?? "";
  const joinedAccounts = $derived(accounts.filter((a) => a.joined));
  // A known GitHub event shows its phrase in the language of the person. Other items show their text.
  // A Telegram chat shows the last message. A message that is not text shows the label of its kind.
  function itemText(item: TimelineItem): string {
    if (item.event) return t(`event_${item.event.kind}` as Key, { repo: item.event.repo });
    if (!item.chat) return item.text;
    let text = item.text;
    if (item.chat.kind) {
      const label = `[${t(`tg_kind_${item.chat.kind}` as Key)}]`;
      text = text ? `${label} ${text}` : label;
    }
    return item.chat.outgoing ? `${t("you")}: ${text}` : text;
  }

  async function load() {
    try {
      accounts = await api.accounts();
      if (accountFilter !== null && !accounts.some((a) => a.id === accountFilter)) accountFilter = null;
      items = await api.timeline(accountFilter, unreadOnly);
      sync = await api.syncState();
    } catch (e) {
      notice = errorText(e);
    }
  }

  $effect(() => {
    void unreadOnly;
    void accountFilter;
    load();
  });

  // The Rust side syncs the joined accounts in the background. It sends an event when something changed.
  $effect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    api.syncNow().catch(() => {});
    listen(TIMELINE_CHANGED, load)
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

  async function syncNow() {
    syncing = true;
    try {
      await api.syncNow();
      await load();
    } catch (e) {
      notice = errorText(e);
    } finally {
      syncing = false;
    }
  }

  // Only an item with a read state can become read. GitHub cannot mark a thread as unread, so PANIM does not offer it.
  async function setRead(item: TimelineItem, read: boolean) {
    if (!item.has_read_state || item.read === read) return;
    try {
      await api.setRead(item.key.account, item.key.source_id, read);
      await load();
    } catch (e) {
      notice = errorText(e);
    }
  }

  async function openItem(item: TimelineItem) {
    notice = "";
    try {
      await api.openLink(item.key.account, item.key.source_id);
      await setRead(item, true);
    } catch (e) {
      notice = errorText(e);
    }
  }

  function toggleAnswer(item: TimelineItem) {
    answerText = "";
    answering = answering === itemId(item) ? null : itemId(item);
  }

  async function sendAnswer(item: TimelineItem) {
    notice = "";
    info = "";
    sending = true;
    try {
      await api.answer(item.key.account, item.key.source_id, answerText);
      answering = null;
      answerText = "";
      info = item.chat ? t("answer_sent_telegram") : t("answer_sent");
      await setRead(item, true);
    } catch (e) {
      notice = errorText(e);
    } finally {
      sending = false;
    }
  }

  /** The text of the warning mark of a joined account, or an empty text. */
  function syncWarning(account: Account, state: SyncView | undefined): string {
    if (!state) return "";
    const telegram = account.platform === "Telegram";
    if (state.needs_login) return telegram ? t("sync_needs_login_telegram") : t("sync_needs_login");
    if (!state.error) return "";
    // A Telegram account has no sync. Its mark tells the state of the client.
    const reason = errorText({ code: state.error, detail: "" });
    return telegram ? reason : t("sync_failed", { reason });
  }

  /**
   * A click on an account opens its window, or shows it if it is open. A joined account also sets the filter of the
   * timeline. The menu "All accounts" in the timeline removes the filter.
   */
  function selectAccount(account: Account) {
    if (!account.browser) {
      view = "timeline";
      accountFilter = account.id;
    }
    // A Telegram account has no window. Its chats are in the timeline and in the chat panel.
    if (account.platform === "Telegram") return;
    openAccount(account);
  }

  // The unread state of the account windows. The Rust side sends an event when it changes.
  async function loadUnread() {
    try {
      unread = await api.unread();
    } catch {
      // The state is not important. The next event tries again.
    }
  }

  $effect(() => {
    let stop: (() => void) | null = null;
    let gone = false;
    loadUnread();
    listen(UNREAD_CHANGED, loadUnread)
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

  /** The text for screen readers and the tooltip of the unread marks. */
  function unreadLabel(state: Unread): string {
    const parts: string[] = [];
    if (state.count > 0) parts.push(t("unread_count", { n: state.count }));
    else if (state.some) parts.push(t("unread_some"));
    if (state.notice) parts.push(t("unread_notice"));
    return parts.join(", ");
  }

  async function openAccount(account: Account) {
    notice = "";
    try {
      await api.openAccount(account.id);
    } catch (e) {
      notice = errorText(e);
    }
  }

  async function removeAccount(account: Account) {
    const texts: Partial<Record<string, Key>> = {
      GitHub: "remove_confirm_github",
      Telegram: "remove_confirm_telegram",
    };
    if (!confirm(t(texts[account.platform] ?? "remove_confirm", { name: account.display_name }))) return;
    if (chatItem?.key.account === account.id) chatItem = null;
    try {
      const removed = await api.removeAccount(account.id);
      if (removed.session_left) info = t("tg_session_left");
    } catch (e) {
      notice = errorText(e);
    }
    load();
  }

  function openChat(item: TimelineItem) {
    notice = "";
    answering = null;
    chatItem = item;
  }

  async function lock() {
    await api.lock();
    onchange();
  }

  // The automatic lock: input in the main window resets the time.
  $effect(() => {
    const minutes = status.settings.auto_lock_minutes;
    if (!minutes) return;
    let timer = setTimeout(lock, minutes * 60_000);
    const reset = () => {
      clearTimeout(timer);
      timer = setTimeout(lock, minutes * 60_000);
    };
    const events = ["pointerdown", "pointermove", "keydown", "wheel"] as const;
    events.forEach((name) => window.addEventListener(name, reset, { passive: true }));
    return () => {
      clearTimeout(timer);
      events.forEach((name) => window.removeEventListener(name, reset));
    };
  });

  const initials = (platform: string) => (platform === "Email" ? "@" : platform.slice(0, 1));
</script>

<div class="shell">
  <aside>
    <div class="brand">
      <Mark size={28} />
      <span class="name">{t("app_name")}</span>
    </div>

    <nav>
      <button class:active={view === "timeline"} onclick={() => (view = "timeline")}>{t("nav_timeline")}</button>
      <button class:active={view === "proofs"} onclick={() => (view = "proofs")}>{t("nav_proofs")}</button>
      <button class:active={view === "settings"} onclick={() => (view = "settings")}>{t("nav_settings")}</button>
    </nav>

    <div class="section-title muted small">{t("accounts_title")}</div>
    <ul class="accounts">
      {#each accounts as account (account.id)}
        {@const state = unread[account.id]}
        {@const joined = sync[account.id]}
        {@const warning = syncWarning(account, joined)}
        {@const telegram = account.platform === "Telegram"}
        <li>
          <button
            class="account"
            class:selected={accountFilter === account.id}
            onclick={() => selectAccount(account)}
            title={account.browser ? t("open") : telegram ? t("show_chats") : t("show_and_open")}
          >
            <span class="badge" data-platform={account.platform}>{initials(account.platform)}</span>
            <span class="who">
              <span class="label">{account.display_name}</span>
              <span class="muted small">{account.handle || t(`platform_${account.platform}` as Key)}</span>
            </span>
            {#if state && (state.count > 0 || state.some || state.notice)}
              <span class="marks" role="img" aria-label={unreadLabel(state)} title={unreadLabel(state)}>
                {#if state.notice}
                  <svg class="notice-mark" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" aria-hidden="true"><path d="M6 8a6 6 0 0112 0c0 7 3 9 3 9H3s3-2 3-9" /><path d="M10.3 21a1.94 1.94 0 003.4 0" /></svg>
                {/if}
                {#if state.count > 0}
                  <span class="count" aria-hidden="true">{state.count > 99 ? "99+" : state.count}</span>
                {:else if state.some}
                  <span class="dot" aria-hidden="true"></span>
                {/if}
              </span>
            {/if}
            {#if joined && (joined.unread > 0 || warning)}
              <span class="marks">
                {#if warning}
                  <span class="warn-mark" role="img" aria-label={warning} title={warning}>!</span>
                {/if}
                {#if joined.unread > 0}
                  {@const shown = joined.unread > 99 ? "99+" : joined.unread_more ? `${joined.unread}+` : joined.unread}
                  {@const label = telegram ? t("tg_unread_chats", { n: shown }) : t("unread_count", { n: shown })}
                  <span class="count" role="img" aria-label={label} title={label}>
                    {shown}
                  </span>
                {/if}
              </span>
            {/if}
          </button>
          <button class="quiet remove" onclick={() => removeAccount(account)} aria-label={t("remove")}>×</button>
        </li>
      {:else}
        <li class="muted small empty">{t("accounts_empty")}</li>
      {/each}
    </ul>
    <button class="add" onclick={() => (adding = true)}>+ {t("add_account")}</button>

    <span class="spacer"></span>
    <button class="quiet lock" onclick={lock}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="11" width="16" height="10" rx="2" /><path d="M8 11V7a4 4 0 018 0v4" /></svg>
      {t("lock")}
    </button>
  </aside>

  <main>
    {#if notice}
      <div class="error notice">{notice} <button class="link" onclick={() => (notice = "")}>{t("close")}</button></div>
    {/if}
    {#if info}
      <div class="info notice" role="status">{info} <button class="link" onclick={() => (info = "")}>{t("close")}</button></div>
    {/if}
    {#if view === "settings"}
      <SettingsPage {status} {onchange} />
    {:else if view === "proofs"}
      <ProofsPage {status} {accounts} onaccounts={load} />
    {:else}
      <div class="timeline">
        <div class="row head">
          <h1>{t("timeline_title")}</h1>
          <span class="spacer"></span>
          {#if joinedAccounts.length > 0}
            <select class="filter small" aria-label={t("filter_account")} bind:value={accountFilter}>
              <option value={null}>{t("all_accounts")}</option>
              {#each joinedAccounts as account (account.id)}
                <option value={account.id}>{account.display_name}</option>
              {/each}
            </select>
          {/if}
          <label class="row small muted"><input type="checkbox" bind:checked={unreadOnly} class="check" /> {t("unread_only")}</label>
          {#if joinedAccounts.length > 0}
            <button class="small" disabled={syncing} onclick={syncNow}>{syncing ? t("syncing") : t("sync_now")}</button>
          {/if}
        </div>
        {#each items as item (itemId(item))}
          <article class:unread={!item.read} class:stateless={!item.has_read_state}>
            <button class="item" onclick={() => setRead(item, true)} title={item.read ? undefined : t("mark_read")}>
              <span class="row small muted">
                <span class="author">{item.author}</span>
                <span>· {accountName(item.key.account)}</span>
                <span class="spacer"></span>
                {#if item.chat && item.chat.unread > 0}
                  {@const n = item.chat.unread > 999 ? "999+" : item.chat.unread}
                  <span class="chat-count" role="img" aria-label={t("tg_chat_unread", { n })} title={t("tg_chat_unread", { n })}>{n}</span>
                {:else if item.chat?.marked}
                  <span class="dot" role="img" aria-label={t("tg_marked_unread")} title={t("tg_marked_unread")}></span>
                {/if}
                <time>{new Date(item.time).toLocaleString()}</time>
              </span>
              <span class="text">{itemText(item)}</span>
            </button>
            <div class="row actions small">
              {#if item.has_link}
                <button class="link" onclick={() => openItem(item)}>{t("open_item")}</button>
              {/if}
              {#if item.chat}
                <button class="link" onclick={() => openChat(item)}>{t("open_chat")}</button>
              {/if}
              {#if item.can_answer}
                <button class="link" onclick={() => toggleAnswer(item)}>{t("answer")}</button>
              {/if}
            </div>
            {#if answering === itemId(item)}
              <div class="answer">
                <textarea
                  rows="4"
                  bind:value={answerText}
                  aria-label={t("answer")}
                  placeholder={t(item.chat ? "answer_placeholder_telegram" : "answer_placeholder", {
                    name: accountName(item.key.account),
                  })}
                ></textarea>
                <div class="row">
                  <span class="spacer"></span>
                  <button class="quiet" onclick={() => (answering = null)}>{t("cancel")}</button>
                  <button class="primary" disabled={sending || !answerText.trim()} onclick={() => sendAnswer(item)}>
                    {sending ? t("sending") : t("send")}
                  </button>
                </div>
              </div>
            {/if}
          </article>
        {:else}
          <div class="empty-state">
            <Mark size={56} />
            <h2>{t("timeline_empty_title")}</h2>
            <p class="muted">{t("timeline_empty_body")}</p>
          </div>
        {/each}
      </div>
    {/if}
  </main>
</div>

{#if adding}
  <AddAccount onclose={() => (adding = false)} onadded={() => ((adding = false), load())} />
{/if}

{#if chatItem}
  {#key itemId(chatItem)}
    <ChatPanel item={chatItem} accountName={accountName(chatItem.key.account)} onclose={() => ((chatItem = null), load())} />
  {/key}
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: 250px 1fr;
    height: 100%;
  }

  aside {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 18px 12px;
    background: var(--surface);
    border-right: 1px solid var(--line);
    overflow-y: auto;
  }

  .brand {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 0 8px 14px;
  }

  .name {
    font-weight: 700;
    letter-spacing: 0.16em;
    font-size: 13px;
  }

  nav {
    display: grid;
    gap: 2px;
  }

  nav button {
    text-align: left;
    border-color: transparent;
    background: transparent;
    color: var(--muted);
  }

  nav button.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 600;
  }

  .section-title {
    padding: 18px 10px 4px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 11px;
  }

  .accounts {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }

  .accounts li {
    display: flex;
    align-items: center;
  }

  .accounts .empty {
    padding: 4px 10px;
  }

  .account {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 10px;
    align-items: center;
    text-align: left;
    border-color: transparent;
    background: transparent;
    padding: 6px 8px;
  }

  .account.selected {
    background: var(--surface-2);
  }

  .who {
    display: grid;
    min-width: 0;
  }

  .who > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    display: grid;
    place-items: center;
    font-weight: 700;
    font-size: 13px;
    background: var(--peri-soft);
    color: var(--peri);
  }

  .badge[data-platform="Email"] {
    background: var(--peach-soft);
    color: var(--peach);
  }

  .marks {
    flex: none;
    margin-left: auto;
    display: flex;
    gap: 6px;
    align-items: center;
  }

  .count {
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 10px;
    display: grid;
    place-items: center;
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    background: var(--peach);
    color: var(--bg);
  }

  .chat-count {
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    display: grid;
    place-items: center;
    font-size: 11px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    background: var(--peri-soft);
    color: var(--peri);
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--peach);
  }

  .notice-mark {
    color: var(--peri);
  }

  .warn-mark {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 12px;
    font-weight: 700;
    background: var(--danger-soft);
    color: var(--danger);
  }

  .remove {
    visibility: hidden;
    padding: 2px 8px;
    font-size: 16px;
  }

  .accounts li:hover .remove {
    visibility: visible;
  }

  .add {
    margin-top: 6px;
    border-style: dashed;
    background: transparent;
    color: var(--muted);
  }

  .lock {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: center;
  }

  main {
    overflow-y: auto;
    position: relative;
  }

  .notice {
    margin: 16px 32px 0;
    display: flex;
    gap: 10px;
    justify-content: space-between;
  }

  .timeline {
    max-width: 720px;
    padding: 28px 32px 60px;
    display: grid;
    gap: 12px;
  }

  .head {
    margin-bottom: 8px;
  }

  .check {
    width: auto;
  }

  article {
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 12px 14px;
    display: grid;
    gap: 6px;
  }

  article.unread {
    border-left: 3px solid var(--peach);
  }

  article.unread .text {
    font-weight: 600;
  }

  article:not(.unread):not(.stateless) .text {
    color: var(--muted);
  }

  .item {
    display: grid;
    gap: 6px;
    width: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    text-align: left;
    font: inherit;
  }

  .item:hover:not(:disabled) {
    background: transparent;
  }

  .item .row {
    display: flex;
    gap: 6px;
  }

  .author {
    font-weight: 600;
  }

  .text {
    overflow-wrap: anywhere;
  }

  .actions {
    gap: 14px;
  }

  .answer {
    display: grid;
    gap: 8px;
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

  .filter {
    width: auto;
  }

  .info {
    color: var(--ok);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 8px 12px;
  }

  .empty-state {
    display: grid;
    justify-items: center;
    text-align: center;
    gap: 12px;
    padding: 70px 20px;
    max-width: 440px;
    margin: 0 auto;
  }
</style>
