<!--
  Settings: the language, the automatic lock, the network route, the Telegram app values, the passcode, the key file,
  the backup and the persona.
-->
<script lang="ts">
  import { untrack } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import KeyFileForm from "./KeyFileForm.svelte";
  import Secret from "./Secret.svelte";
  import WordGrid from "./WordGrid.svelte";
  import { api, errorText, MIN_PASSCODE, type ProxyKind, type Status, type TelegramApp } from "./api";
  import { lang, t } from "./i18n.svelte";

  let { status, onchange }: { status: Status; onchange: () => void } = $props();

  const lockChoices = [5, 15, 30, 60, 0];

  // The route form starts from the saved route.
  const route = untrack(() => status.persona?.route ?? "Direct");
  let kind = $state<"Direct" | ProxyKind>(route === "Direct" ? "Direct" : route.Proxy.kind);
  let host = $state(route === "Direct" ? "" : route.Proxy.host);
  let port = $state(route === "Direct" ? "" : String(route.Proxy.port));
  let routeNote = $state("");

  let current = $state("");
  let next = $state("");
  let again = $state("");
  let passNote = $state("");
  let passError = $state("");

  let keyNote = $state("");

  let wordsPasscode = $state("");
  let shownWords = $state<string[]>([]);
  let wordsError = $state("");

  async function showWords() {
    wordsError = "";
    try {
      shownWords = await api.recoveryWords(wordsPasscode);
      wordsPasscode = "";
    } catch (e) {
      wordsError = errorText(e);
    }
  }
  let copied = $state(false);

  // The Rust side can do two calls of setSettings in a different order (milestone 16). Thus, each call waits for
  // the previous call, and the last choice of the person stays.
  let settingsQueue: Promise<unknown> = Promise.resolve();

  // The local copy of the settings. A click changes it at once. `status` changes only after `onchange()`, so a
  // second click before that time must not use the old `status.settings` (milestone 16, review).
  let settings = $state<Status["settings"]>({ ...untrack(() => status.settings) });
  // The number of saves that are not complete. This value is not reactive.
  let pending = 0;

  // A new status replaces the local copy only when no save is in work.
  $effect(() => {
    const fresh = status.settings;
    if (pending === 0) settings = { ...fresh };
  });

  async function saveSettings(change: Partial<Status["settings"]>) {
    settings = { ...settings, ...change };
    const copy = { ...settings };
    pending++;
    const next = settingsQueue.then(() => api.setSettings(copy));
    settingsQueue = next.catch(() => {});
    try {
      await next;
    } finally {
      pending--;
      // Only the last save gets a new status, so that an old status cannot come after it. After a save that
      // failed, the new status puts back the saved settings.
      if (pending === 0) onchange();
    }
  }

  async function setLanguage(code: "en" | "zh") {
    lang.current = code;
    await saveSettings({ language: code });
  }

  async function setLock(minutes: number) {
    await saveSettings({ auto_lock_minutes: minutes });
  }

  async function applyRoute() {
    routeNote = "";
    try {
      await api.setRoute(kind === "Direct" ? "Direct" : { Proxy: { kind, host: host.trim(), port: Number(port) || 0 } });
      routeNote = t("saved");
      onchange();
    } catch (e) {
      routeNote = errorText(e);
    }
  }

  async function changePasscode() {
    passNote = passError = "";
    if (next !== again) return (passError = t("passcode_mismatch"));
    try {
      await api.changePasscode(current, next);
      current = next = again = "";
      passNote = t("saved");
    } catch (e) {
      passError = errorText(e);
    }
  }

  // The Telegram app values. The api_hash never comes back from the Rust side.
  let tgApp = $state<TelegramApp | null>(null);
  let tgOwn = $state(false);
  let apiId = $state("");
  let apiHash = $state("");
  let tgNote = $state("");
  let tgError = $state("");

  function showApp(app: TelegramApp) {
    tgApp = app;
    tgOwn = app.use_own || !app.builtin;
  }

  // A change of the values starts a new number. Then the first answer of telegramApp() cannot replace the answer of
  // the change, if it comes later (milestone 16).
  let tgSeq = 0;
  {
    const seq = tgSeq;
    api.telegramApp().then(
      (app) => seq === tgSeq && showApp(app),
      (e) => seq === tgSeq && (tgError = errorText(e)),
    );
  }

  const typedValues = $derived(apiId.trim() !== "" || apiHash.trim() !== "");

  async function saveTelegramApp() {
    tgSeq++;
    tgNote = tgError = "";
    if (typedValues) {
      if (!/^\d{1,10}$/.test(apiId.trim())) return (tgError = t("tg_values_bad_id"));
      if (!/^[0-9a-fA-F]{32}$/.test(apiHash.trim())) return (tgError = t("tg_values_bad_hash"));
    }
    try {
      showApp(await api.setTelegramApp(tgOwn, typedValues ? apiId : null, typedValues ? apiHash : null));
      apiId = apiHash = "";
      tgNote = t("saved");
    } catch (e) {
      tgError = errorText(e);
    }
  }

  async function forgetTelegramApp() {
    tgSeq++;
    tgNote = tgError = "";
    try {
      showApp(await api.forgetTelegramApp());
      apiId = apiHash = "";
      tgNote = t("saved");
    } catch (e) {
      tgError = errorText(e);
    }
  }

  // The backup of the vault.
  let backupBusy = $state(false);
  let backupNote = $state("");
  let backupError = $state("");

  async function backup() {
    backupNote = backupError = "";
    const day = new Date().toISOString().slice(0, 10);
    const path = await save({
      title: t("backup_dialog_title"),
      defaultPath: `panim-backup-${day}.panim`,
      filters: [{ name: t("backup_filter"), extensions: ["panim"] }],
    });
    if (!path) return;
    backupBusy = true;
    try {
      const bytes = await api.backupVault(path);
      backupNote = t("backup_done", { size: `${(bytes / 1048576).toFixed(1)} MB` });
    } catch (e) {
      backupError = errorText(e);
    } finally {
      backupBusy = false;
    }
  }

  async function copy() {
    await navigator.clipboard.writeText(status.persona?.address ?? "");
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="page">
  <h1>{t("settings_title")}</h1>

  <section>
    <h3>{t("settings_persona")}</h3>
    <p class="muted small">{t("persona_body")}</p>
    <div class="row">
      <code class="mono address">{status.persona?.address}</code>
      <button class="quiet small" onclick={copy}>{copied ? t("copied") : t("copy")}</button>
    </div>
  </section>

  <section>
    <h3>{t("settings_language")}</h3>
    <div class="row">
      <button class:on={lang.current === "en"} onclick={() => setLanguage("en")}>{t("lang_en")}</button>
      <button class:on={lang.current === "zh"} onclick={() => setLanguage("zh")}>{t("lang_zh")}</button>
    </div>
  </section>

  <section>
    <h3>{t("settings_autolock")}</h3>
    <div class="row wrap">
      {#each lockChoices as m (m)}
        <button class:on={settings.auto_lock_minutes === m} onclick={() => setLock(m)}>
          {m === 0 ? t("autolock_never") : t("minutes", { n: m })}
        </button>
      {/each}
    </div>
  </section>

  <section>
    <h3>{t("settings_route")}</h3>
    <p class="muted small">{t("route_body")}</p>
    <div class="row wrap">
      <button class:on={kind === "Direct"} onclick={() => (kind = "Direct")}>{t("route_direct")}</button>
      <button class:on={kind === "Http"} onclick={() => (kind = "Http")}>{t("route_http")}</button>
      <button class:on={kind === "Socks5"} onclick={() => (kind = "Socks5")}>{t("route_socks")}</button>
    </div>
    {#if kind !== "Direct"}
      <div class="proxy">
        <label class="field">{t("host")}<input bind:value={host} placeholder="127.0.0.1" spellcheck="false" /></label>
        <label class="field">{t("port")}<input bind:value={port} placeholder="9050" inputmode="numeric" /></label>
      </div>
      <p class="muted small">{t("route_tor_hint")}</p>
      <p class="muted small">{t("route_telegram_note")}</p>
    {/if}
    <div class="row">
      <button onclick={applyRoute}>{t("apply")}</button>
      <span class="muted small">{routeNote}</span>
    </div>
  </section>

  <section>
    <h3>{t("settings_telegram")}</h3>
    <p class="muted small">{t("tg_values_body")}</p>
    {#if tgApp}
      <div class="row wrap">
        <button class:on={!tgOwn} disabled={!tgApp.builtin} onclick={() => (tgOwn = false)}>{t("tg_values_builtin")}</button>
        <button class:on={tgOwn} onclick={() => (tgOwn = true)}>{t("tg_values_own")}</button>
      </div>
      {#if tgApp.builtin}
        <p class="muted small">{t("tg_values_builtin_body")}</p>
      {:else}
        <p class="warn small">{t("tg_values_builtin_none")}</p>
      {/if}
      {#if tgOwn}
        <p class="muted small">{t("tg_values_where")}</p>
        <p class="warn small">{t("tg_values_link_warning")}</p>
        {#if tgApp.own_api_id !== null}
          <p class="small">{t("tg_values_set", { id: tgApp.own_api_id })} {t("tg_values_replace")}</p>
        {/if}
        <div class="values">
          <label class="field">{t("tg_api_id")}<input bind:value={apiId} inputmode="numeric" autocomplete="off" spellcheck="false" /></label>
          <label class="field">
            {t("tg_api_hash")}<input bind:value={apiHash} type="password" autocomplete="off" spellcheck="false" />
          </label>
        </div>
      {/if}
      <p class="muted small">{t("tg_values_only_new")}</p>
      {#if tgError}<div class="error">{tgError}</div>{/if}
      <div class="row">
        <button onclick={saveTelegramApp}>{t("tg_values_save")}</button>
        {#if tgApp.own_api_id !== null}
          <button class="quiet" onclick={forgetTelegramApp}>{t("tg_values_forget")}</button>
        {/if}
        <span class="muted small">{tgNote}</span>
      </div>
      <p class="small">
        {#if tgApp.next_login === "own"}
          {t("tg_next_own", { id: tgApp.own_api_id ?? "" })}
        {:else if tgApp.next_login === "builtin"}
          {t("tg_next_builtin")}
        {:else}
          {t("tg_next_none")}
        {/if}
      </p>
    {:else if tgError}
      <div class="error">{tgError}</div>
    {/if}
  </section>

  <section>
    <h3>{t("settings_passcode")}</h3>
    <div class="fields">
      <Secret label={t("current_passcode")} bind:value={current} autocomplete="current-password" />
      <Secret label={t("new_passcode")} bind:value={next} autocomplete="new-password" hint={t("passcode_hint", { n: MIN_PASSCODE })} />
      <Secret label={t("passcode_again")} bind:value={again} autocomplete="new-password" />
    </div>
    {#if passError}<div class="error">{passError}</div>{/if}
    <div class="row">
      <button disabled={!current || next.length < MIN_PASSCODE || next !== again} onclick={changePasscode}>{t("change")}</button>
      <span class="muted small">{passNote}</span>
    </div>
  </section>

  <section>
    <h3>{t("settings_words")}</h3>
    {#if shownWords.length}
      <WordGrid words={shownWords} />
      <p class="warn small">{t("words_warning")}</p>
      <div class="row"><button onclick={() => (shownWords = [])}>{t("hide_words")}</button></div>
    {:else}
      <p class="muted small">{t("settings_words_body")}</p>
      <div class="fields">
        <Secret label={t("passcode")} bind:value={wordsPasscode} autocomplete="current-password" onenter={showWords} />
      </div>
      {#if wordsError}<div class="error">{wordsError}</div>{/if}
      <div class="row"><button disabled={!wordsPasscode} onclick={showWords}>{t("show_words")}</button></div>
    {/if}
  </section>

  <section>
    <h3>{t("settings_keyfile")}</h3>
    <p class="muted small">{t("keyfile_optional_body")}</p>
    <div class="fields">
      <KeyFileForm onsaved={() => (keyNote = t("keyfile_saved"))} />
    </div>
    <span class="muted small">{keyNote}</span>
  </section>

  <section>
    <h3>{t("settings_backup")}</h3>
    <p class="muted small">{t("backup_body")}</p>
    <p class="muted small">{t("backup_note")}</p>
    <p class="warn small">{t("backup_warning")}</p>
    {#if backupError}<div class="error">{backupError}</div>{/if}
    <div class="row">
      <button disabled={backupBusy} onclick={backup}>{backupBusy ? t("backup_busy") : t("backup_action")}</button>
      <span class="muted small">{backupNote}</span>
    </div>
  </section>
</div>

<style>
  .page {
    max-width: 620px;
    padding: 28px 32px 60px;
    display: grid;
    gap: 26px;
  }

  section {
    display: grid;
    gap: 10px;
    padding-top: 20px;
    border-top: 1px solid var(--line);
  }

  .wrap {
    flex-wrap: wrap;
  }

  button.on {
    border-color: var(--peri);
    background: var(--peri-soft);
    font-weight: 600;
  }

  .address {
    background: var(--surface-2);
    padding: 6px 10px;
    border-radius: 6px;
  }

  .proxy {
    display: grid;
    grid-template-columns: 1fr 120px;
    gap: 10px;
  }

  .warn {
    color: var(--peach);
  }

  .values {
    display: grid;
    grid-template-columns: 140px 1fr;
    gap: 10px;
  }

  .fields {
    display: grid;
    gap: 10px;
    max-width: 360px;
  }
</style>
