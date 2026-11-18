<!-- The gate: the passcode, or the recovery with the 12 words or a key file. After 5 wrong tries, only the recovery
     is left. -->
<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import Card from "./Card.svelte";
  import Secret from "./Secret.svelte";
  import { api, errorText, MIN_PASSCODE } from "./api";
  import { t } from "./i18n.svelte";

  let { locked, onchange }: { locked: boolean; onchange: () => void } = $props();

  let recovering = $state(false);
  let passcode = $state("");
  let busy = $state(false);
  let error = $state("");
  let shake = $state(false);

  let method = $state<"words" | "file">("words");
  let words = $state("");
  let keyFile = $state("");
  let password = $state("");
  let newPasscode = $state("");
  let again = $state("");

  const showRecover = $derived(locked || recovering);
  const proofReady = $derived(method === "words" ? words.trim().split(/\s+/).length === 12 : !!(keyFile && password));
  const recoverReady = $derived(proofReady && newPasscode.length >= MIN_PASSCODE && newPasscode === again);

  async function unlock() {
    if (!passcode) return;
    error = "";
    busy = true;
    try {
      await api.unlock(passcode);
      onchange();
    } catch (e) {
      error = errorText(e);
      passcode = "";
      shake = true;
      setTimeout(() => (shake = false), 400);
      onchange();
    } finally {
      busy = false;
    }
  }

  async function choose() {
    const path = await open({ title: t("recover_dialog_title"), filters: [{ name: "JSON", extensions: ["json"] }] });
    if (typeof path === "string") keyFile = path;
  }

  async function recover() {
    error = "";
    if (newPasscode !== again) return (error = t("passcode_mismatch"));
    busy = true;
    try {
      if (method === "words") await api.recoverWithWords(words, newPasscode);
      else await api.recover(keyFile, password, newPasscode);
      words = "";
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<Card>
  {#if !showRecover}
    <h1>{t("gate_title")}</h1>
    <div class:shake>
      <Secret label={t("passcode")} bind:value={passcode} autofocus autocomplete="current-password" onenter={unlock} />
    </div>
    {#if error}<div class="error">{error}</div>{/if}
    <button class="primary" disabled={!passcode || busy} onclick={unlock}>{busy ? t("unlocking") : t("unlock")}</button>
    <button class="link small" onclick={() => ((recovering = true), (error = ""))}>{t("forgot")}</button>
  {:else}
    {#if locked}
      <h1>{t("gate_locked_title")}</h1>
      <p class="muted">{t("gate_locked_body")}</p>
    {:else}
      <h1>{t("recover_title")}</h1>
      <p class="muted small">{t("recover_body")}</p>
    {/if}

    <div class="tabs">
      <button class:on={method === "words"} onclick={() => (method = "words")}>{t("recover_with_words")}</button>
      <button class:on={method === "file"} onclick={() => (method = "file")}>{t("recover_with_file")}</button>
    </div>

    {#if method === "words"}
      <label class="field">
        {t("recover_words_label")}
        <textarea bind:value={words} rows="3" spellcheck="false" autocomplete="off" autocapitalize="off"></textarea>
        <span class="small">{t("recover_words_hint")}</span>
      </label>
    {:else}
      <div class="row">
        <button onclick={choose}>{t("recover_choose")}</button>
        <span class="muted small file">{keyFile ? keyFile.split("/").pop() : t("recover_no_file")}</span>
      </div>
      <Secret label={t("keyfile_password")} bind:value={password} autocomplete="off" />
    {/if}

    <Secret label={t("new_passcode")} bind:value={newPasscode} autocomplete="new-password" hint={t("passcode_hint", { n: MIN_PASSCODE })} />
    <Secret label={t("passcode_again")} bind:value={again} autocomplete="new-password" onenter={() => recoverReady && recover()} />
    {#if error}<div class="error">{error}</div>{/if}
    <div class="row">
      {#if !locked}
        <button class="quiet" onclick={() => ((recovering = false), (error = ""))}>{t("back")}</button>
      {/if}
      <span class="spacer"></span>
      <button class="primary" disabled={!recoverReady || busy} onclick={recover}>
        {busy ? t("recovering") : t("recover_button")}
      </button>
    </div>
  {/if}
</Card>

<style>
  .file {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    padding: 4px;
    background: var(--surface-2);
    border-radius: 10px;
  }

  .tabs button {
    border-color: transparent;
    background: transparent;
    color: var(--muted);
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--text);
    font-weight: 600;
    border-color: var(--line);
  }

  textarea {
    font: 14px/1.6 var(--mono);
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 8px;
    padding: 8px 10px;
    resize: vertical;
  }

  textarea:focus-visible {
    outline: none;
    box-shadow: var(--focus);
  }

  .shake {
    animation: shake 0.35s;
  }

  @keyframes shake {
    20%,
    60% {
      transform: translateX(-6px);
    }
    40%,
    80% {
      transform: translateX(6px);
    }
  }
</style>
