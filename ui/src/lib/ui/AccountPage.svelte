<!-- Settings › Account (docs/devel/token.md): who is signed in, where the token comes from, and what
     Tsugiki does with it, as ADR 0005 has it. -->
<script lang="ts">
  import type { About } from "$lib/state/about.svelte";
  import { AccountState, signedIn, tokenReadme } from "$lib/state/account.svelte";
  import { LinkOpener } from "$lib/state/link-opener.svelte";
  import type { Status } from "$lib/state/status.svelte";
  import { linkTooltip } from "./link-tooltip";
  import { TOKEN_SOURCE_WORDS } from "./token-words";

  let { status, about }: { status: Status | undefined; about: About | undefined } = $props();

  const account = new AccountState();
  void account.load();
  const who = $derived(signedIn(status));
  const readme = $derived(tokenReadme(about));

  /** Opens the README, and keeps what kept the browser from it until the next try. */
  const readmePage = new LinkOpener();
  /** Opens the README in the system browser. Enter on the link arrives as a click. */
  async function followReadme(e: MouseEvent) {
    e.preventDefault();
    await readmePage.openLink(readme);
  }
  /** A middle click opens it too, and would otherwise ask the webview for a window. The right button and
   * the mouse's back and forward buttons leave it alone. */
  function followReadmeByWheel(e: MouseEvent) {
    if (e.button === 1) void followReadme(e);
  }
</script>

<!-- eslint-disable svelte/no-navigation-without-resolve -- The link opens in the system browser through
     openLink, and doesn't move the window. -->

<h3>GitHub</h3>
<dl>
  <dt>Signed in as</dt>
  <dd>
    {#if who.login !== undefined}<b>{who.login}</b>{/if}
    {#if who.note !== undefined}<small>{who.note}</small>{/if}
  </dd>
  <dt>Token from</dt>
  <dd>
    {#if account.source !== undefined}
      <b>{account.source}</b>
      <small>{TOKEN_SOURCE_WORDS[account.source]}</small>
    {:else if account.error !== undefined}
      <small>{`Tsugiki can't say: ${account.error}`}</small>
    {/if}
  </dd>
</dl>

<h3>What Tsugiki does with the token</h3>
<ul>
  <li>
    Only Tsugiki's core, written in Rust, holds it. The window shows PRs and comments that anyone can write,
    so it never gets the token, and reaches the network only for the images GitHub serves for them.
  </li>
  <li>It goes only to <code>api.github.com</code>, with Tsugiki's requests to GitHub's API.</li>
  <li>It is never written to the settings files, the database or the log.</li>
</ul>
<p>
  The code that holds and sends the token is a crate of its own, small enough to check, and its
  <a href={readme} title={linkTooltip(readme)} onclick={followReadme} onauxclick={followReadmeByWheel}
    >README</a
  > says what it does in plain words.
</p>
{#if readmePage.problem !== undefined}
  <p role="alert">{readmePage.problem}</p>
{/if}

<style>
  /* A label column and a value column, as the prototype's Account page lays them out. */
  dl {
    display: grid;
    grid-template-columns: 120px 1fr;
    gap: 8px 14px;
    margin: 0;
  }

  dt {
    color: var(--fg-muted);
    text-align: right;
  }

  dd {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
  }

  small {
    font-size: 12px;
    color: var(--fg-muted);
  }

  ul {
    display: grid;
    gap: 4px;
    margin: 0;
    padding-left: 18px;
  }

  p {
    margin: 10px 0 0;
  }

  a {
    color: var(--accent);
  }
</style>
