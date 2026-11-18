<!-- The proof screen: the persona address, a proof for an account of the person, and the check of a proof of a
     different persona. All texts from a gist or from a pasted text show as text, never as HTML. -->
<script lang="ts">
  import { untrack } from "svelte";
  import {
    api,
    errorText,
    MAX_PASTE,
    type Account,
    type AppError,
    type BlockView,
    type GistCheck,
    type PastedView,
    type ProofView,
    type Status,
  } from "./api";
  import { t, type Key } from "./i18n.svelte";

  let { status, accounts, onaccounts }: { status: Status; accounts: Account[]; onaccounts: () => void } = $props();

  // The address in the form of the proof block: 0x and 40 lowercase hexadecimal digits.
  const address = $derived((status.persona?.address ?? "").toLowerCase());

  let proofs = $state<ProofView[]>([]);
  let selected = $state<number | null>(null);
  let error = $state("");
  let info = $state("");
  let busy = $state(false);
  // A copy shows "Copied" for a short time. The key tells which copy button.
  let copied = $state("");

  // The form for the handle and the numeric ID of a browser-view account.
  let editing = $state(false);
  let handle = $state("");
  let numericId = $state("");
  let location = $state("");
  let gist = $state("");

  const account = $derived(accounts.find((a) => a.id === selected) ?? null);
  const proof = $derived(proofs.find((p) => p.account === selected) ?? null);
  const proofOf = (id: number) => proofs.find((p) => p.account === id);

  async function loadProofs() {
    try {
      proofs = await api.proofs();
    } catch (e) {
      error = errorText(e);
    }
  }

  // The list of accounts changes when the person adds or removes an account.
  $effect(() => {
    void accounts;
    untrack(() => {
      loadProofs();
      if (selected !== null && !accounts.some((a) => a.id === selected)) selected = null;
    });
  });

  function choose(next: Account) {
    selected = next.id;
    error = info = "";
    handle = next.handle;
    numericId = next.remote_id ?? "";
    location = proofOf(next.id)?.location ?? "";
    gist = "";
    editing = false;
  }

  async function copy(key: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = key;
      setTimeout(() => (copied = copied === key ? "" : copied), 1500);
    } catch {
      // The text stays on the screen, and the person can select it.
    }
  }

  async function run(work: () => Promise<void>) {
    error = info = "";
    busy = true;
    try {
      await work();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  const saveIdentity = (target: Account) =>
    run(async () => {
      await api.setIdentity(target.id, handle, numericId);
      editing = false;
      info = t("proofs_identity_saved");
      onaccounts();
      await loadProofs();
    });

  const make = (target: Account) =>
    run(async () => {
      try {
        await api.makeProof(target.id, false);
      } catch (e) {
        const failure = e as Partial<AppError>;
        if (failure?.code !== "proof_replace") throw e;
        if (!confirm(t("proofs_replace_confirm", { date: failure.detail ?? "" }))) return;
        await api.makeProof(target.id, true);
      }
      location = "";
      await loadProofs();
    });

  const saveLocation = (target: Account) =>
    run(async () => {
      const view = await api.setProofLocation(target.id, location);
      location = view.location ?? "";
      info = t("saved");
      await loadProofs();
    });

  const checkOwn = (target: Account) =>
    run(async () => {
      const result = await api.checkOwnProof(target.id, gist);
      location = result.proof.location ?? "";
      info = t("proofs_own_passed");
      await loadProofs();
    });

  const openTarget = (target: Account) =>
    run(async () => {
      if (target.platform === "GitHub") await api.openGistPage();
      else await api.openAccount(target.id);
    });

  // Part 2: the check of a proof of a different persona.
  let mode = $state<"gist" | "paste">("gist");
  let examineInput = $state("");
  let pasted = $state("");
  let examineError = $state("");
  let examining = $state(false);
  let gistResult = $state<GistCheck | null>(null);
  let pastedResult = $state<PastedView | null>(null);

  function setMode(next: "gist" | "paste") {
    mode = next;
    examineError = "";
  }

  async function examine() {
    examineError = "";
    gistResult = null;
    pastedResult = null;
    examining = true;
    try {
      if (mode === "gist") gistResult = await api.checkGithubProof(examineInput);
      else pastedResult = await api.checkPastedProof(pasted);
    } catch (e) {
      examineError = errorText(e);
    } finally {
      examining = false;
    }
  }

  /** True if the gist has a different correct block for the same persona, so the state mark is useful. */
  const hasOthers = (check: GistCheck, block: BlockView) =>
    check.blocks.filter((b) => b.current !== null && b.persona === block.persona).length > 1;

  const kindText = (domain: string | null) =>
    domain === "revoke" ? t("examine_kind_revoke") : domain === "proof" ? t("examine_kind_proof") : "";
</script>

<div class="page">
  <h1>{t("proofs_title")}</h1>
  <p class="muted">{t("proofs_intro")}</p>

  <section aria-labelledby="proofs-address">
    <h3 id="proofs-address">{t("proofs_address")}</h3>
    <p class="muted small">{t("proofs_address_body")}</p>
    <div class="row">
      <code class="mono address">{address}</code>
      <button class="quiet small" onclick={() => copy("address", address)}>
        {copied === "address" ? t("copied") : t("copy")}
      </button>
    </div>
    <p class="warn small" role="note">{t("proofs_v1_notice")}</p>
  </section>

  <section aria-labelledby="proofs-make">
    <h3 id="proofs-make">{t("proofs_make")}</h3>
    {#if accounts.length === 0}
      <p class="muted small">{t("accounts_empty")}</p>
    {:else}
      <p class="muted small">{t("proofs_make_body")}</p>
      <ul class="choices">
        {#each accounts as item (item.id)}
          {@const state = proofOf(item.id)}
          <li>
            <button class="choice" class:on={selected === item.id} aria-pressed={selected === item.id} onclick={() => choose(item)}>
              <span class="who">
                <span class="label">{item.display_name}</span>
                <span class="muted small">{t(`platform_${item.platform}` as Key)} · {item.handle || "—"}</span>
              </span>
              <span class="small" class:muted={!state?.date} class:made={!!state?.date}>
                {state?.date ? t("proofs_made_on", { date: state.date }) : t("proofs_none_yet")}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    {#if error}<div class="error" role="alert">{error}</div>{/if}
    {#if info}<div class="ok small" role="status">{info}</div>{/if}

    {#if account && proof}
      {#if proof.identity_editable}
        {#if proof.id_needed || editing}
          <div class="box">
            <h3>{t("proofs_identity")}</h3>
            <p class="muted small">
              {account.platform === "Email" ? t("proofs_identity_body_email") : t("proofs_identity_body")}
            </p>
            <div class="fields">
              <label class="field">
                {t("proofs_handle")}
                <input bind:value={handle} spellcheck="false" autocomplete="off" placeholder={account.platform === "Email" ? "name@proton.me" : "name"} />
              </label>
              {#if account.platform !== "Email"}
                <label class="field">
                  {t("proofs_numeric_id")}
                  <input bind:value={numericId} inputmode="numeric" spellcheck="false" autocomplete="off" placeholder="1234567890" />
                  <span class="small muted">
                    {account.platform === "X"
                      ? t("proofs_id_help_X")
                      : account.platform === "Discord"
                        ? t("proofs_id_help_Discord")
                        : t("proofs_id_help_other")}
                  </span>
                </label>
              {/if}
            </div>
            {#if proof.block}<p class="warn small">{t("proofs_identity_drops")}</p>{/if}
            <div class="row">
              <button disabled={busy} onclick={() => saveIdentity(account)}>{t("proofs_location_save")}</button>
              {#if editing}<button class="quiet" onclick={() => (editing = false)}>{t("cancel")}</button>{/if}
            </div>
          </div>
        {:else}
          <div class="row">
            <span class="muted small">{account.handle}{account.remote_id ? ` · ${account.remote_id}` : ""}</span>
            <button class="link small" onclick={() => (editing = true)}>{t("proofs_identity_change")}</button>
          </div>
        {/if}
      {/if}

      {#if !proof.id_needed}
        <div class="row">
          <button class="primary" disabled={busy} onclick={() => make(account)}>
            {proof.block ? t("proofs_make_again") : t("proofs_make_button")}
          </button>
        </div>
      {/if}

      {#if proof.block}
        <div class="box">
          <div class="row">
            <h3>{t("proofs_block")}</h3>
            <span class="spacer"></span>
            <button class="small" onclick={() => copy("block", proof.block ?? "")}>
              {copied === "block" ? t("copied") : t("copy")}
            </button>
          </div>
          <textarea
            class="mono block"
            readonly
            rows="8"
            spellcheck="false"
            value={proof.block}
            aria-label={t("proofs_block_label", { name: account.display_name })}
          ></textarea>
          <p class="small"><strong>{t("proofs_you_post")}</strong></p>
          <dl class="small">
            <dt>{t("proofs_where")}</dt>
            <dd>{t(`proofs_where_${account.platform}` as Key)}</dd>
            <dt>{t("proofs_strength")}</dt>
            <dd>{t(`proofs_strength_${account.platform}` as Key)}</dd>
          </dl>
          {#if account.platform === "GitHub" || account.browser}
            <div class="row">
              <button disabled={busy} onclick={() => openTarget(account)}>
                {account.platform === "GitHub" ? t("proofs_open_gist") : t("proofs_open_account")}
              </button>
            </div>
          {/if}

          <h3>{t("proofs_location")}</h3>
          {#if proof.location}
            <code class="mono location">{proof.location}</code>
          {:else}
            <p class="muted small">{t("proofs_location_none")}</p>
          {/if}
          {#if account.platform === "GitHub"}
            <div class="row">
              <label class="field grow">
                {t("proofs_gist_label")}
                <input bind:value={gist} spellcheck="false" autocomplete="off" placeholder="https://gist.github.com/…" />
              </label>
            </div>
            <div class="row">
              <button disabled={busy || !gist.trim()} onclick={() => checkOwn(account)}>
                {busy ? t("proofs_checking") : t("proofs_check_own")}
              </button>
            </div>
          {:else}
            <div class="row">
              <label class="field grow">
                {t("proofs_location_label")}
                <input bind:value={location} spellcheck="false" autocomplete="off" placeholder="https://…" />
              </label>
            </div>
            <div class="row">
              <button disabled={busy} onclick={() => saveLocation(account)}>{t("proofs_location_save")}</button>
            </div>
          {/if}
        </div>
      {/if}
    {/if}
  </section>

  <section aria-labelledby="proofs-examine">
    <h3 id="proofs-examine">{t("examine_title")}</h3>
    <p class="muted small">{t("examine_body")}</p>
    <div class="row" role="group" aria-label={t("examine_title")}>
      <button class:on={mode === "gist"} aria-pressed={mode === "gist"} onclick={() => setMode("gist")}>{t("examine_gist")}</button>
      <button class:on={mode === "paste"} aria-pressed={mode === "paste"} onclick={() => setMode("paste")}>{t("examine_paste")}</button>
    </div>

    {#if mode === "gist"}
      <label class="field">
        {t("examine_gist_label")}
        <input
          bind:value={examineInput}
          spellcheck="false"
          autocomplete="off"
          placeholder="https://gist.github.com/…"
          onkeydown={(e) => e.key === "Enter" && examineInput.trim() && examine()}
        />
      </label>
    {:else}
      <label class="field">
        {t("examine_paste_label")}
        <textarea rows="8" class="mono" bind:value={pasted} maxlength={MAX_PASTE} spellcheck="false"></textarea>
        <span class="small muted">{t("examine_paste_hint")}</span>
      </label>
    {/if}
    <div class="row">
      <button
        class="primary"
        disabled={examining || (mode === "gist" ? !examineInput.trim() : !pasted.trim())}
        onclick={examine}
      >
        {examining ? t("proofs_checking") : t("examine_check")}
      </button>
    </div>
    {#if examineError}<div class="error" role="alert">{examineError}</div>{/if}

    {#if mode === "gist" && gistResult}
      {@const check = gistResult}
      <div class="box" role="status">
        <p class="small">
          {check.owner ? t("examine_owner", { login: check.owner.login, id: check.owner.id }) : t("examine_owner_none")}
        </p>
        {#if check.result === "secret"}
          <p class="warn small">{t("examine_secret")}</p>
        {:else if check.result === "no_block"}
          <p class="warn small">{t("examine_no_block")}</p>
        {:else}
          {#each check.blocks as block, index (index)}
            <article class="result" class:good={block.verdict === "correct"}>
              <p class="small" class:ok={block.verdict === "correct"} class:bad={block.verdict !== "correct"}>
                {t(`verdict_${block.verdict}` as Key)}
              </p>
              <dl class="small">
                <dt>{t("examine_file")}</dt>
                <dd class="mono">{block.file}</dd>
                {#if block.persona}
                  <dt>{t("examine_persona")}</dt>
                  <dd class="mono">{block.persona}</dd>
                  <dt>{t("examine_kind")}</dt>
                  <dd>{kindText(block.domain)}</dd>
                  <dt>{t("examine_date")}</dt>
                  <dd>{block.date}</dd>
                  <dt>{t("examine_platform")}</dt>
                  <dd>{block.platform}</dd>
                  <dt>{t("examine_id")}</dt>
                  <dd class="mono">{block.id ?? "-"}</dd>
                  <dt>{t("examine_handle")}</dt>
                  <dd class="mono">{block.handle ?? "-"}</dd>
                {/if}
              </dl>
              {#if block.persona === address}<p class="small own">{t("examine_own")}</p>{/if}
              {#if block.current === true && block.domain === "revoke"}
                <p class="warn small"><strong>{t("examine_revoked", { date: block.date ?? "" })}</strong></p>
              {/if}
              {#if block.current !== null && hasOthers(check, block)}
                <p class="small" class:muted={!block.current}>
                  {block.current ? t("examine_current") : t("examine_old")}
                </p>
              {/if}
            </article>
          {/each}
          {#if check.cut}<p class="muted small">{t("examine_cut", { n: check.blocks.length })}</p>{/if}
        {/if}
      </div>
    {/if}

    {#if mode === "paste" && pastedResult}
      {@const block = pastedResult}
      <div class="box" role="status">
        <p class="small ok">{t("examine_paste_ok")}</p>
        <dl class="small">
          <dt>{t("examine_persona")}</dt>
          <dd class="mono">{block.persona}</dd>
          <dt>{t("examine_kind")}</dt>
          <dd>{kindText(block.domain)}</dd>
          <dt>{t("examine_date")}</dt>
          <dd>{block.date}</dd>
          <dt>{t("examine_platform")}</dt>
          <dd>{block.platform}</dd>
          <dt>{t("examine_id")}</dt>
          <dd class="mono">{block.id ?? "-"}</dd>
          <dt>{t("examine_handle")}</dt>
          <dd class="mono">{block.handle ?? "-"}</dd>
        </dl>
        {#if block.own}<p class="small own">{t("examine_own")}</p>{/if}
        {#if block.domain === "revoke"}
          <p class="warn small"><strong>{t("examine_revoked", { date: block.date })}</strong></p>
        {/if}
        <p class="warn small">{t("examine_paste_not_checked")}</p>
        <ol class="small">
          <li>
            {block.id
              ? t("examine_compare_account_id", { id: block.id })
              : t("examine_compare_account_handle", { handle: block.handle ?? "-" })}
          </li>
          <li>{t("examine_compare_platform", { platform: block.platform })}</li>
        </ol>
      </div>
    {/if}
  </section>
</div>

<style>
  .page {
    max-width: 680px;
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

  button.on {
    border-color: var(--peri);
    background: var(--peri-soft);
    font-weight: 600;
  }

  .address,
  .location {
    background: var(--surface-2);
    padding: 6px 10px;
    border-radius: 6px;
  }

  .warn {
    color: var(--peach);
  }

  .ok {
    color: var(--ok);
  }

  .bad {
    color: var(--danger);
  }

  .own {
    color: var(--peri);
    font-weight: 600;
  }

  .made {
    color: var(--ok);
  }

  .choices {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }

  .choice {
    width: 100%;
    display: flex;
    gap: 10px;
    align-items: center;
    justify-content: space-between;
    text-align: left;
  }

  .who {
    display: grid;
    min-width: 0;
  }

  .who > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .box {
    display: grid;
    gap: 10px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    padding: 14px 16px;
  }

  .fields {
    display: grid;
    gap: 10px;
    max-width: 420px;
  }

  .grow {
    flex: 1;
  }

  textarea.block {
    background: var(--surface-2);
    word-break: break-all;
    resize: none;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 2px 14px;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  ol {
    margin: 0;
    padding-left: 20px;
  }

  .result {
    display: grid;
    gap: 6px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-left: 3px solid var(--danger);
    border-radius: 8px;
  }

  .result.good {
    border-left-color: var(--ok);
  }

  textarea {
    width: 100%;
    padding: 8px 10px;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: 8px;
    resize: vertical;
  }

  textarea:focus-visible {
    outline: none;
    box-shadow: var(--focus);
  }
</style>
