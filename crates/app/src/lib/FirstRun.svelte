<!--
  The first start, steps 1 and 2: the language, then the passcode. Steps 3 and 4, the recovery words, are `Backup`.
  Step 1 also offers the restore of a backup. After the restore, the gate opens the restored vault.
-->
<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import Card from "./Card.svelte";
  import Secret from "./Secret.svelte";
  import { api, errorText, MIN_PASSCODE, type Settings } from "./api";
  import { lang, t } from "./i18n.svelte";

  let { settings, onchange }: { settings: Settings; onchange: () => void } = $props();

  let step = $state(1);
  let restoring = $state(false);
  let passcode = $state("");
  let again = $state("");
  let busy = $state(false);
  let error = $state("");

  const ready = $derived(passcode.length >= MIN_PASSCODE && passcode === again);

  async function language(code: "en" | "zh") {
    lang.current = code;
    await api.setSettings({ ...settings, language: code });
  }

  async function restore() {
    error = "";
    const path = await open({ title: t("restore_dialog_title"), filters: [{ name: t("backup_filter"), extensions: ["panim"] }] });
    if (typeof path !== "string") return;
    busy = true;
    try {
      await api.restoreVault(path);
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  async function create() {
    error = "";
    if (passcode !== again) return (error = t("passcode_mismatch"));
    busy = true;
    try {
      await api.create(passcode);
      passcode = again = "";
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<Card step={restoring ? undefined : step}>
  {#if restoring}
    <h1>{t("restore_title")}</h1>
    <p class="muted">{t("restore_body")}</p>
    <p class="warn small">{t("restore_warning")}</p>
    {#if error}<div class="error">{error}</div>{/if}
    <div class="row">
      <button class="quiet" disabled={busy} onclick={() => ((restoring = false), (error = ""))}>{t("back")}</button>
      <span class="spacer"></span>
      <button class="primary" disabled={busy} onclick={restore}>{busy ? t("restore_busy") : t("restore_select")}</button>
    </div>
  {:else if step === 1}
    <h1>{t("first_title")}</h1>
    <p class="muted">{t("first_intro")}</p>
    <div class="stack">
      <span class="muted small">{t("first_language")}</span>
      <div class="langs">
        <button class:on={lang.current === "en"} onclick={() => language("en")}>{t("lang_en")}</button>
        <button class:on={lang.current === "zh"} onclick={() => language("zh")}>{t("lang_zh")}</button>
      </div>
    </div>
    <button class="primary" onclick={() => (step = 2)}>{t("next")}</button>
    <button class="quiet" onclick={() => (restoring = true)}>{t("restore_link")}</button>
  {:else}
    <h1>{t("passcode_set_title")}</h1>
    <p class="muted">{t("passcode_set_body")}</p>
    <Secret label={t("passcode")} bind:value={passcode} autofocus autocomplete="new-password" hint={t("passcode_hint", { n: MIN_PASSCODE })} />
    <Secret label={t("passcode_again")} bind:value={again} autocomplete="new-password" onenter={() => ready && create()} />
    {#if error}<div class="error">{error}</div>{/if}
    <div class="row">
      <button class="quiet" disabled={busy} onclick={() => (step = 1)}>{t("back")}</button>
      <span class="spacer"></span>
      <button class="primary" disabled={!ready || busy} onclick={create}>{busy ? t("creating") : t("next")}</button>
    </div>
  {/if}
</Card>

<style>
  .langs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }

  .langs button {
    padding: 12px;
  }

  .warn {
    color: var(--peach);
  }

  .langs button.on {
    border-color: var(--peri);
    background: var(--peri-soft);
    font-weight: 600;
  }
</style>
