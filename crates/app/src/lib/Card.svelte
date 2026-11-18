<!-- The frame of the screens before the app opens: the first start, the gate and the recovery. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Mark from "./Mark.svelte";
  import { t } from "./i18n.svelte";

  let { step, children }: { step?: number; children: Snippet } = $props();
</script>

<div class="page">
  <div class="brand">
    <Mark size={44} />
    <div>
      <div class="name">{t("app_name")}</div>
      <div class="muted small">{t("tagline")}</div>
    </div>
  </div>
  <section class="card">
    {#if step}
      <div class="step muted small">{t("step", { n: step })}</div>
    {/if}
    {@render children()}
  </section>
</div>

<style>
  .page {
    min-height: 100%;
    display: grid;
    place-content: center;
    gap: 22px;
    padding: 40px 20px;
  }

  .brand {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: center;
  }

  .name {
    font-weight: 700;
    letter-spacing: 0.16em;
    font-size: 15px;
  }

  .card {
    width: min(440px, 92vw);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 28px;
    display: grid;
    gap: 16px;
    box-shadow: 0 10px 40px -20px rgb(0 0 0 / 0.35);
  }

  .step {
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-size: 11px;
  }
</style>
