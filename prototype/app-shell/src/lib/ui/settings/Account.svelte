<script lang="ts">
  // PROTOTYPE — throwaway. Settings › Account: who is signed in, where the token comes from, what it
  // lets gh-rev do (in plain words, per Repository), what gh-rev does with it, and guides for making a
  // narrower token with GitHub's form already filled in. The select at the bottom stands in for GitHub.
  import { settingDef, type SettingDef } from "$lib/core/settings";
  import { capabilities, GUIDES } from "$lib/core/tokens";
  import { value, secret } from "$lib/state/settings.svelte";
  import { REPOS } from "$lib/state/app.svelte";
  import { account, token, hasToken, me, simulate, SIM, refusedNow, forgetRefusals } from "$lib/state/account.svelte";
  import { openInBrowser } from "$lib/platform/shell";
  import Field from "./Field.svelte";
  import Where from "./Where.svelte";
  import { setValue } from "$lib/state/settings.svelte";

  const t = $derived(token());
  const can = $derived(capabilities(t, REPOS, refusedNow()));
  const owners = [...new Set(REPOS.map((r) => r.split("/")[0]))];
  let guide = $state(GUIDES[0].key);
  let owner = $state(owners.find((o) => o !== me()) ?? owners[0]);
  const g = $derived(GUIDES.find((x) => x.key === guide)!);
  const fine = $derived(value("auth.source") === "token" && secret.token.startsWith("github_pat_"));
  const d = (k: string) => settingDef(k)!;
  const defs = $derived([d("auth.source"), d("auth.token"), ...(fine ? [d("auth.fine_allows"), ...(value("auth.fine_allows") === "write" ? [d("auth.fine_owner")] : [])] : [])]);
  const MARK = { ok: "✓", no: "✕", warn: "⚠", unknown: "?" };
  const source = $derived(value("auth.source") !== "token" ? "the gh CLI (gh auth token)" : !hasToken() ? "" : t.kind === "classic" ? "a classic personal access token (ghp_…)" : "a fine-grained personal access token (github_pat_…)");

  function openForm() {
    // A fine-grained token pasted next is assumed to be the one this guide made.
    account.template = guide === "read" ? { allows: "read", owner: me() } : guide === "review" ? { allows: "write", owner } : null;
    if (value("auth.source") !== "token") setValue("auth.source", "token");
    openInBrowser(g.url(owner));
  }
  $effect(() => {
    if (account.template && secret.token.startsWith("github_pat_")) {
      setValue("auth.fine_allows", account.template.allows);
      setValue("auth.fine_owner", account.template.owner === me() ? "" : account.template.owner);
      account.template = null;
    }
  });
</script>

<h3>GitHub</h3>
{#if hasToken()}
  <p class="status">✓ Signed in as <b>{me()}</b> with {source}</p>
{:else}
  <p class="status warn">No token yet: paste one below, or make a read-only one with the guide further down. Until then every Repository is Read-only.</p>
{/if}
<div class="form">
  {#each defs as def (def.key)}
    <label class="lab" for="f-{def.key}">{def.label}</label>
    <div class="ctl"><Field {def} radios /> <Where where={def.where} /><div class="help">{def.help}</div></div>
  {/each}
</div>

<h3>What this token lets gh-rev do</h3>
<ul class="can">
  {#each can as c}<li class={c.mark}><span class="m">{MARK[c.mark]}</span><span>{c.text}</span></li>{/each}
</ul>
{#if t.kind !== "fine"}<p class="help">Scopes, as GitHub reports them: <span class="mono">{t.scopes.join(", ")}</span>{#if t.kind === "gh"}&nbsp;(the same as <span class="mono">gh auth status</span> shows){/if}.
  {#if t.kind === "gh" && t.scopes.includes("repo")}gh-rev only needs to read, comment and review: a narrower token below is safer to try it with.{/if}</p>{/if}
{#if refusedNow().length}<p class="help">If the token changed on GitHub since it was refused (approved, or given more access), <button class="link" onclick={forgetRefusals}>try writing again</button>.</p>{/if}

<h3>What gh-rev does with it</h3>
<ul class="trust">
  <li>Only gh-rev's core holds the token. The pages that show PRs and comments, written by anyone, never see it and can't reach the network.</li>
  <li>It goes to <span class="mono">api.github.com</span> and <span class="mono">github.com</span> only. No telemetry, no other servers.</li>
  <li>gh's token is read from <span class="mono">gh</span> at each start and never stored. A pasted token is kept only in the OS keychain, never in config.toml or local.toml.</li>
  <li>The git mirror of a public Repository is fetched without it.</li>
  <li>The source is public, and each release can be checked with <span class="mono">gh attestation verify</span>.</li>
</ul>

<h3>Make a narrower token</h3>
<div class="tabs" role="tablist">
  {#each GUIDES as x}<button role="tab" class:on={guide === x.key} aria-selected={guide === x.key} onclick={() => (guide = x.key)}>{x.title}</button>{/each}
</div>
<div class="guide">
  <p class="pitch">{g.pitch}</p>
  {#if guide === "review"}
    <p class="owner">For the repositories of
      {#each owners as o}<label><input type="radio" name="owner" checked={owner === o} onchange={() => (owner = o)} /> <span class="mono">{o}</span></label>{/each}
      <span class="help">A fine-grained token writes to one owner's repositories only.</span></p>
  {/if}
  <ol>{#each g.steps(owner) as s}<li>{s}</li>{/each}</ol>
  <div class="cc">
    <div><b>Can</b><ul>{#each g.can as c}<li>✓ {c}</li>{/each}</ul></div>
    <div><b>Can't</b><ul>{#each g.cannot as c}<li>✕ {c}</li>{/each}</ul></div>
  </div>
  {#if g.note?.(owner, owner === me())}<p class="note">⚠ {g.note(owner, owner === me())}</p>{/if}
  <p class="open"><button class="primary" onclick={openForm}>Open GitHub's form ↗</button>
    <span class="mono url" title={g.url(owner)}>{g.url(owner).replace("https://", "")}</span></p>
</div>

<div class="proto">
  <b>PROTOTYPE</b> · there's no GitHub here. Pretend the token is:
  <select value={account.sim} onchange={(e) => simulate(e.currentTarget.value)}>
    {#each SIM as [k, label]}<option value={k}>{label}</option>{/each}
  </select>
  <span class="help">(or <span class="mono">?token={SIM.map(([k]) => k).join("|")}</span>)</span>
</div>

<style>
  h3 { font-size: 13px; margin: 16px 0 8px; padding-bottom: 4px; border-bottom: 1px solid var(--line); }
  .status { margin: 0 0 12px; color: var(--good); }
  .status.warn { color: var(--warn); }
  .form { display: grid; grid-template-columns: 170px 1fr; gap: 10px 14px; }
  .lab { text-align: right; padding-top: 4px; }
  .help { color: var(--muted); font-size: 12px; margin-top: 3px; }
  .mono { font-family: var(--mono); font-size: 12px; }
  ul { margin: 0; padding: 0; list-style: none; }
  .can li { display: flex; gap: 8px; padding: 2px 0; }
  .can .m { width: 14px; flex: none; text-align: center; font-weight: 700; }
  .can .ok .m { color: var(--good); }
  .can .no .m { color: var(--muted); }
  .can .warn { color: var(--warn); }
  .can .unknown .m { color: var(--accent); }
  .link { all: unset; cursor: pointer; color: var(--accent); }
  .trust { list-style: disc; padding-left: 18px; display: grid; gap: 3px; }
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); }
  .tabs button { all: unset; cursor: pointer; padding: 5px 10px; border: 1px solid transparent; border-bottom: 0; border-radius: 5px 5px 0 0; color: var(--muted); margin-bottom: -1px; }
  .tabs button.on { border-color: var(--line); background: var(--bg); color: var(--fg); font-weight: 600; }
  .guide { border: 1px solid var(--line); border-top: 0; border-radius: 0 0 6px 6px; padding: 4px 14px 12px; }
  .pitch { margin: 8px 0; }
  .owner { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin: 6px 0; }
  .owner label { display: inline-flex; gap: 4px; align-items: center; cursor: pointer; }
  ol { margin: 6px 0; padding-left: 20px; display: grid; gap: 3px; }
  .cc { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; margin: 8px 0; font-size: 12.5px; }
  .cc li { padding: 1px 0; }
  .note { color: var(--warn); font-size: 12.5px; margin: 6px 0; }
  .open { display: flex; gap: 10px; align-items: center; margin: 10px 0 0; min-width: 0; }
  .url { color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; font-size: 11px; }
  .primary { background: var(--accent); color: var(--on-accent); border-color: var(--accent); flex: none; }
  .proto { margin-top: 18px; padding: 8px 10px; border: 1px dashed var(--line); border-radius: 6px; font-size: 12px; display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
</style>
