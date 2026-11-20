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

  // The number of the latest call of refresh(). A slow older call must not replace the answer of a newer call.
  let refreshSeq = 0;

  async function refresh() {
    const seq = ++refreshSeq;
    try {
      const next = await api.status();
      if (seq !== refreshSeq) return;
      status = next;
      lang.current = next.settings.language;
    } catch (e) {
      if (seq === refreshSeq) failure = errorText(e);
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
