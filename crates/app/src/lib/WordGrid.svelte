<!-- The 12 recovery words, numbered, in three columns. -->
<script lang="ts">
  let { words }: { words: string[] } = $props();
</script>

<ol class="grid">
  {#each words as word, i (i)}
    <li><span class="n">{i + 1}</span><span class="w">{word}</span></li>
  {/each}
</ol>

<style>
  .grid {
    list-style: none;
    margin: 0;
    padding: 12px;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px 10px;
    background: var(--surface-2);
    border-radius: var(--radius);
    user-select: text;
  }

  li {
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 4px 6px;
  }

  .n {
    width: 1.6em;
    text-align: right;
    color: var(--muted);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }

  .w {
    font-family: var(--mono);
    font-size: 14px;
    font-weight: 600;
  }
</style>
