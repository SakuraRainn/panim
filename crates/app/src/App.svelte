<!-- Selects the screen from the phase that the Rust side gives. -->
<script lang="ts">
  import FirstRun from "./lib/FirstRun.svelte";
  import Gate from "./lib/Gate.svelte";
  import Main from "./lib/Main.svelte";
  import Backup from "./lib/Backup.svelte";
  import { api, errorText, type Status } from "./lib/api";
  import { lang } from "./lib/i18n.svelte";

  let status = $state<Status | null>(null);
  let failure = $state("");

  async function refresh() {
    try {
      status = await api.status();
      lang.current = status.settings.language;
    } catch (e) {
      failure = errorText(e);
    }
  }

  refresh();
</script>

{#if failure}
  <div class="failure error">{failure}</div>
{:else if status}
  {#if status.phase === "first_run"}
    <FirstRun settings={status.settings} onchange={refresh} />
  {:else if status.phase === "backup"}
    <Backup onchange={refresh} />
  {:else if status.phase === "locked" || status.phase === "gate_locked"}
    <Gate locked={status.phase === "gate_locked"} onchange={refresh} />
  {:else}
    <Main {status} onchange={refresh} />
  {/if}
{/if}

<style>
  .failure {
    margin: 40px;
  }
</style>
