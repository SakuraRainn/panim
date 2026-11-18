<!--
  The dialog to add an account: a GitHub account through the device login, a Telegram account through the phone
  number, the code and the two-step password, or a browser-view account.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    api,
    errorText,
    type AppError,
    type LoginView,
    type Platform,
    type TelegramLogin,
    type TelegramStatus,
  } from "./api";
  import { t, type Key } from "./i18n.svelte";

  let { onclose, onadded }: { onclose: () => void; onadded: () => void } = $props();

  const platforms: Platform[] = ["GitHub", "Telegram", "X", "Discord", "Email"];
  let platform = $state<Platform>("GitHub");
  let displayName = $state("");
  let handle = $state("");
  let busy = $state(false);
  let error = $state("");

  // The device login of GitHub. The device code stays on the Rust side.
  let login = $state<LoginView | null>(null);
  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function add() {
    error = "";
    busy = true;
    try {
      await api.addBrowserAccount(platform, displayName, handle);
      onadded();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  function schedule(seconds: number) {
    clearTimeout(timer);
    timer = setTimeout(poll, Math.max(1, seconds) * 1000);
  }

  async function startLogin() {
    error = "";
    busy = true;
    try {
      login = await api.githubLoginStart();
      schedule(login.interval);
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function poll() {
    const current = login;
    if (!current) return;
    try {
      const result = await api.githubLoginPoll();
      if (login !== current) return;
      if ("done" in result) {
        login = null;
        onadded();
      } else {
        error = "";
        schedule(result.pending.interval);
      }
    } catch (e) {
      if (login !== current) return;
      error = errorText(e);
      // A network failure does not stop the login. All other errors stop it.
      if ((e as Partial<AppError>)?.code === "network") schedule(current.interval);
      else login = null;
    }
  }

  function cancelLogin() {
    clearTimeout(timer);
    if (!login) return;
    login = null;
    api.githubLoginCancel().catch(() => {});
  }

  function choose(next: Platform) {
    if (next === platform) return;
    cancelLogin();
    cancelTelegram();
    error = "";
    platform = next;
    if (next === "Telegram") loadTelegram();
  }

  async function openGithub() {
    try {
      await api.openGithubLogin();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function copyCode() {
    if (!login) return;
    try {
      await navigator.clipboard.writeText(login.user_code);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // The code stays on the screen, and the person can select it.
    }
  }

  // The login of Telegram. The phone number, the code and the password go to the Rust side only. The Rust side does
  // not send them back.
  let telegram = $state<TelegramStatus | null>(null);
  let tgLogin = $state<TelegramLogin | null>(null);
  let phone = $state("");
  let code = $state("");
  let password = $state("");
  // The number of the code step, as the person typed it. It stays in this dialog only, so that the person can see a
  // wrong digit. It does not go to a log or to the vault.
  let codePhone = $state("");
  // True while the person types a different number in the code step.
  let changingNumber = $state(false);
  // The time (milliseconds) when Telegram permits a resend, and the current time for the countdown.
  let resendAt = $state(0);
  let now = $state(Date.now());
  const resendLeft = $derived(Math.max(0, Math.ceil((resendAt - now) / 1000)));

  // The countdown of the resend runs only in the code step.
  $effect(() => {
    if (tgLogin?.state !== "wait_code") return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  async function loadTelegram() {
    try {
      telegram = await api.telegramStatus();
    } catch (e) {
      error = errorText(e);
    }
  }

  /** Runs one step of the Telegram login. A complete login closes the dialog. Returns true if the step passed. */
  async function tgStep(step: () => Promise<TelegramLogin>): Promise<boolean> {
    error = "";
    busy = true;
    try {
      const next = await step();
      // The secrets of the step are not necessary after the step.
      code = "";
      password = "";
      if (next.account) {
        tgLogin = null;
        onadded();
      } else {
        tgLogin = next;
        changingNumber = false;
        // Each new code state starts the timeout of Telegram again.
        if (next.state === "wait_code") resendAt = Date.now() + next.timeout * 1000;
      }
      return true;
    } catch (e) {
      error = errorText(e);
      // These errors stop the login on the Rust side.
      const stopped = ["telegram_login_none", "telegram_proxy_name", "telegram_not_configured", "account_exists"];
      if (stopped.includes((e as Partial<AppError>)?.code ?? "")) tgLogin = null;
      return false;
    } finally {
      busy = false;
    }
  }

  /** Gives the phone number to Telegram: the first time, or a different number in the code step. */
  async function submitPhone() {
    const typed = phone.trim();
    if (await tgStep(() => api.telegramLoginPhone(typed))) codePhone = typed;
  }

  function cancelTelegram() {
    if (!tgLogin) return;
    tgLogin = null;
    phone = "";
    codePhone = "";
    changingNumber = false;
    code = "";
    password = "";
    api.telegramLoginCancel().catch(() => {});
  }

  function close() {
    cancelLogin();
    cancelTelegram();
    onclose();
  }

  // The dialog can go away without a click, for example when the app locks.
  onDestroy(() => {
    cancelLogin();
    cancelTelegram();
  });
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="dialog" role="dialog" aria-modal="true" tabindex="-1" onkeydown={(e) => e.key === "Escape" && close()}>
    <h2>{t("add_title")}</h2>
    <div class="platforms">
      {#each platforms as p (p)}
        <button class:on={platform === p} onclick={() => choose(p)}>{t(`platform_${p}` as Key)}</button>
      {/each}
    </div>
    {#if platform === "GitHub"}
      <p class="muted small">{t("github_add_body")}</p>
      {#if login}
        <div class="code-box">
          <span class="muted small">{t("github_code_label")}</span>
          <div class="row">
            <span class="code mono">{login.user_code}</span>
            <span class="spacer"></span>
            <button onclick={copyCode}>{copied ? t("copied") : t("copy")}</button>
          </div>
        </div>
        <p class="small">{t("github_steps")}</p>
        <p class="muted small" role="status">{t("github_waiting")}</p>
      {/if}
      {#if error}<div class="error">{error}</div>{/if}
      <div class="row">
        <span class="spacer"></span>
        <button class="quiet" onclick={close}>{t("cancel")}</button>
        {#if login}
          <button class="primary" onclick={openGithub}>{t("github_open")}</button>
        {:else}
          <button class="primary" disabled={busy} onclick={startLogin}>
            {busy ? t("github_starting") : t("github_start")}
          </button>
        {/if}
      </div>
    {:else if platform === "Telegram"}
      <p class="muted small">{t("tg_add_body")}</p>
      <p class="muted small">{t("tg_count_note")}</p>
      <p class="small note">{t("tg_phone_note")}</p>
      {#if telegram?.proxy}
        <p class="small warn">{t("tg_proxy_warning")}</p>
      {/if}
      {#if telegram && !telegram.configured}
        <div class="error">{t("tg_not_configured")}</div>
      {:else if telegram?.values}
        <p class="small note">
          {telegram.values === "own" ? t("tg_login_values_own") : t("tg_login_values_builtin")}
        </p>
      {/if}
      {#if !telegram || !telegram.configured}
        <!-- No login without app values. -->
      {:else if tgLogin?.state === "wait_phone"}
        <form class="grid" onsubmit={(e) => (e.preventDefault(), submitPhone())}>
          <label class="field">
            {t("tg_phone")}
            <input bind:value={phone} type="tel" autocomplete="off" spellcheck="false" placeholder="+1 555 000 1111" />
          </label>
        </form>
      {:else if tgLogin?.state === "wait_code" && changingNumber}
        <form class="grid" onsubmit={(e) => (e.preventDefault(), submitPhone())}>
          <label class="field">
            {t("tg_phone")}
            <input bind:value={phone} type="tel" autocomplete="off" spellcheck="false" placeholder="+1 555 000 1111" />
          </label>
        </form>
      {:else if tgLogin?.state === "wait_code"}
        {@const login = tgLogin}
        <p class="small">
          {t(`tg_code_via_${login.via}` as Key)}
          {#if login.detail && (login.via === "sms_word" || login.via === "sms_phrase")}
            {t("tg_code_starts", { detail: login.detail })}
          {:else if login.detail && login.via === "flash_call"}
            {t("tg_code_pattern", { detail: login.detail })}
          {:else if login.detail && login.via === "missed_call"}
            {t("tg_code_prefix", { detail: login.detail })}
          {/if}
        </p>
        {#if codePhone}
          <p class="small row">
            <span>{t("tg_code_number", { phone: codePhone })}</span>
            <button class="link" onclick={() => ((changingNumber = true), (error = ""))}>{t("tg_code_other_number")}</button>
          </p>
        {/if}
        <form class="grid" onsubmit={(e) => (e.preventDefault(), tgStep(() => api.telegramLoginCode(code)))}>
          <label class="field">
            {t("tg_code")}
            <input
              bind:value={code}
              autocomplete="one-time-code"
              spellcheck="false"
              maxlength={login.length > 0 ? login.length : 64}
            />
          </label>
        </form>
        <div class="resend">
          {#if login.next}
            <span class="muted small">{t(`tg_next_${login.next}` as Key)}</span>
            <button disabled={busy || resendLeft > 0} onclick={() => tgStep(api.telegramLoginResend)}>
              {resendLeft > 0 ? t("tg_resend_wait", { n: resendLeft }) : t("tg_resend")}
            </button>
          {:else}
            <span class="muted small">{t("tg_resend_no_route")}</span>
          {/if}
        </div>
      {:else if tgLogin?.state === "wait_password"}
        <form class="grid" onsubmit={(e) => (e.preventDefault(), tgStep(() => api.telegramLoginPassword(password)))}>
          <label class="field">
            {t("tg_password")}
            <input bind:value={password} type="password" autocomplete="off" />
          </label>
          {#if tgLogin.hint}<span class="muted small">{t("tg_password_hint", { hint: tgLogin.hint })}</span>{/if}
        </form>
      {:else if tgLogin?.state === "unsupported"}
        <div class="error">{t(`tg_unsupported_${tgLogin.reason}` as Key)}</div>
      {/if}
      {#if error}<div class="error">{error}</div>{/if}
      {#if busy && tgLogin}<p class="muted small" role="status">{t("tg_working")}</p>{/if}
      <div class="row">
        <span class="spacer"></span>
        <button class="quiet" onclick={close}>{t("cancel")}</button>
        {#if telegram?.configured}
          {#if !tgLogin}
            <button class="primary" disabled={busy} onclick={() => tgStep(api.telegramLoginStart)}>
              {busy ? t("tg_starting") : t("tg_start")}
            </button>
          {:else if tgLogin.state === "wait_phone" || (tgLogin.state === "wait_code" && changingNumber)}
            {#if changingNumber}
              <button class="quiet" disabled={busy} onclick={() => (changingNumber = false)}>{t("back")}</button>
            {/if}
            <button class="primary" disabled={busy || !phone.trim()} onclick={submitPhone}>
              {t("tg_send_phone")}
            </button>
          {:else if tgLogin.state === "wait_code"}
            <button class="primary" disabled={busy || !code.trim()} onclick={() => tgStep(() => api.telegramLoginCode(code))}>
              {t("tg_log_in")}
            </button>
          {:else if tgLogin.state === "wait_password"}
            <button class="primary" disabled={busy || !password} onclick={() => tgStep(() => api.telegramLoginPassword(password))}>
              {t("tg_log_in")}
            </button>
          {/if}
        {/if}
      </div>
    {:else}
      <p class="muted small">{t("add_body")}</p>
      <label class="field">
        {t("handle")}
        <input bind:value={handle} spellcheck="false" placeholder={platform === "Email" ? "name@proton.me" : "@name"} />
      </label>
      <label class="field">
        <span>{t("display_name")} <span class="small">({t("display_name_hint")})</span></span>
        <input bind:value={displayName} placeholder={t(`platform_${platform}` as Key)} />
      </label>
      {#if error}<div class="error">{error}</div>{/if}
      <p class="muted small">{t("coming_soon")}</p>
      <div class="row">
        <span class="spacer"></span>
        <button class="quiet" onclick={close}>{t("cancel")}</button>
        <button class="primary" disabled={busy} onclick={add}>{t("add")}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.4);
    display: grid;
    place-items: center;
    z-index: 10;
  }

  .dialog {
    width: min(460px, 92vw);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 24px;
    display: grid;
    gap: 14px;
  }

  .platforms {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 8px;
  }

  .platforms button {
    padding: 10px 6px;
  }

  .platforms button.on {
    border-color: var(--peri);
    background: var(--peri-soft);
    font-weight: 600;
  }

  .code-box {
    display: grid;
    gap: 4px;
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .grid {
    display: grid;
    gap: 6px;
  }

  .resend {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    justify-content: space-between;
  }

  .note {
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--surface-2);
  }

  .warn {
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--peach-soft);
  }

  .code {
    font-size: 28px;
    font-weight: 700;
    letter-spacing: 0.12em;
    user-select: all;
  }
</style>
