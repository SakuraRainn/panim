<!-- Saves the key file of the persona key: a password twice, then a save dialog. -->
<script lang="ts">
  import { save } from "@tauri-apps/plugin-dialog";
  import Secret from "./Secret.svelte";
  import { api, errorText, MIN_PASSCODE } from "./api";
  import { t } from "./i18n.svelte";

  let { onsaved }: { onsaved: () => void } = $props();

  let password = $state("");
  let again = $state("");
  let busy = $state(false);
  let error = $state("");

  const ready = $derived(password.length >= MIN_PASSCODE && password === again);

  async function run() {
    error = "";
    if (password !== again) return (error = t("passcode_mismatch"));
    if (password.length < MIN_PASSCODE) return (error = t("err_passcode_too_short", { n: MIN_PASSCODE }));
    const day = new Date().toISOString().slice(0, 10);
    const path = await save({
      title: t("keyfile_dialog_title"),
      defaultPath: `panim-key-${day}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    busy = true;
    try {
      await api.exportKeyFile(password, path);
      password = again = "";
      onsaved();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="stack">
  <Secret label={t("keyfile_password")} bind:value={password} autocomplete="new-password" hint={t("keyfile_password_hint", { n: MIN_PASSCODE })} />
  <Secret label={t("passcode_again")} bind:value={again} autocomplete="new-password" onenter={run} />
  {#if error}<div class="error">{error}</div>{/if}
  <button class="primary" disabled={!ready || busy} onclick={run}>
    {busy ? t("keyfile_saving") : t("keyfile_save")}
  </button>
</div>
