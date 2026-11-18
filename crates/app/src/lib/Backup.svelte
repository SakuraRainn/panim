<!-- The first start, steps 3 and 4: write down the 12 recovery words, then check four of them. -->
<script lang="ts">
  import Card from "./Card.svelte";
  import WordGrid from "./WordGrid.svelte";
  import { api, errorText } from "./api";
  import { t } from "./i18n.svelte";

  let { onchange }: { onchange: () => void } = $props();

  let words = $state<string[]>([]);
  let checking = $state(false);
  let error = $state("");
  let busy = $state(false);

  // Four different places, from 1 to 12, in order.
  const places = (() => {
    const all = Array.from({ length: 12 }, (_, i) => i + 1);
    for (let i = all.length - 1; i > 0; i--) {
      const j = crypto.getRandomValues(new Uint32Array(1))[0] % (i + 1);
      [all[i], all[j]] = [all[j], all[i]];
    }
    return all.slice(0, 4).sort((a, b) => a - b);
  })();
  let answers = $state(["", "", "", ""]);

  api.recoveryWords(null).then(
    (w) => (words = w),
    (e) => (error = errorText(e)),
  );

  async function check() {
    error = "";
    busy = true;
    try {
      await api.checkWords(places.map((place, i) => [place, answers[i]] as [number, string]));
      words = [];
      onchange();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if !checking}
  <Card step={3}>
    <h1>{t("words_title")}</h1>
    <p class="muted">{t("words_body")}</p>
    <WordGrid {words} />
    <p class="warn small">{t("words_warning")}</p>
    {#if error}<div class="error">{error}</div>{/if}
    <button class="primary" disabled={words.length !== 12} onclick={() => (checking = true)}>{t("words_written")}</button>
  </Card>
{:else}
  <Card step={4}>
    <h1>{t("check_title")}</h1>
    <p class="muted">{t("check_body")}</p>
    <div class="answers">
      {#each places as place, i (place)}
        <label class="field">
          {t("word_n", { n: place })}
          <input
            bind:value={answers[i]}
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            onkeydown={(e) => e.key === "Enter" && check()}
          />
        </label>
      {/each}
    </div>
    {#if error}<div class="error">{error}</div>{/if}
    <div class="row">
      <button class="quiet" onclick={() => ((checking = false), (error = ""))}>{t("back")}</button>
      <span class="spacer"></span>
      <button class="primary" disabled={busy || answers.some((a) => !a.trim())} onclick={check}>{t("check_button")}</button>
    </div>
  </Card>
{/if}

<style>
  .warn {
    color: var(--peach);
  }

  .answers {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .answers input {
    font-family: var(--mono);
  }
</style>
