<!-- A field for a passcode or a password, with a button to show the text. -->
<script lang="ts">
  let {
    label,
    value = $bindable(""),
    hint = "",
    autofocus = false,
    autocomplete = "off",
    onenter,
  }: {
    label: string;
    value?: string;
    hint?: string;
    autofocus?: boolean;
    autocomplete?: "off" | "new-password" | "current-password";
    onenter?: () => void;
  } = $props();

  let shown = $state(false);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (autofocus) input?.focus();
  });
</script>

<label class="field">
  {label}
  <span class="wrap">
    <input
      bind:this={input}
      bind:value
      type={shown ? "text" : "password"}
      {autocomplete}
      spellcheck="false"
      onkeydown={(e) => e.key === "Enter" && onenter?.()}
    />
    <button type="button" class="quiet eye" tabindex="-1" onclick={() => (shown = !shown)} aria-label="show">
      {#if shown}
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 3l18 18M10.6 10.6a2 2 0 002.8 2.8M9.9 5.1A10 10 0 0112 5c6 0 10 7 10 7a17 17 0 01-3.2 4M6.6 6.6A17 17 0 002 12s4 7 10 7a10 10 0 005.4-1.6" /></svg>
      {:else}
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z" /><circle cx="12" cy="12" r="3" /></svg>
      {/if}
    </button>
  </span>
  {#if hint}<span class="small">{hint}</span>{/if}
</label>

<style>
  .wrap {
    position: relative;
    display: block;
  }

  .wrap input {
    padding-right: 40px;
  }

  .eye {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    padding: 5px 7px;
    line-height: 0;
  }
</style>
