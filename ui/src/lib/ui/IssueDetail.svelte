<script lang="ts">
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { IssueViewState } from "$lib/state/issue-view.svelte";
  import RenderedHtml from "./RenderedHtml.svelte";
  import { ago, backportWords, headline, prStateWords } from "./workbench-row";

  let { repository, issue, now }: { repository: string; issue: IssueViewState; now: Date } = $props();

  let browserProblem = $state<string>();

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(repository, item);
  };
  const followLink = async (address: string) => {
    browserProblem = await openLink(address);
  };
  const earlierComments = (count: number) =>
    count === 1 ? "1 earlier comment is on GitHub." : `${String(count)} earlier comments are on GitHub.`;
  const openPr = (number: number) => () => void onGitHub({ kind: "pull_request", number });
</script>

<section class="detail" aria-label="Issue view">
  {#if issue.error}
    <p role="alert">{`Tsugiki can't show this Issue: ${issue.error}`}</p>
  {/if}
  {#if browserProblem}
    <p role="alert">{browserProblem}</p>
  {/if}
  {#if issue.view === null}
    <p>{`Tsugiki doesn't hold Issue #${String(issue.number)}.`}</p>
  {:else if issue.view}
    {@const view = issue.view}
    <h2>
      <button class="link" onclick={() => void onGitHub({ kind: "issue", number: view.number })}>
        {headline(view)}
      </button>
    </h2>
    <p class="by">{view.author ?? "deleted user"}</p>

    {#if view.pull_requests.length > 0}
      <ul class="prs">
        {#each view.pull_requests as entry (entry.number)}
          <li>
            <button class="link" onclick={openPr(entry.number)}>{headline(entry)}</button>
            <small>{prStateWords(entry)}</small>
            {#if entry.kind === "pr" && entry.backports.length > 0}
              <small class="backports">
                Backports:
                {#each entry.backports as backport (backport.number)}
                  <button class="link" onclick={openPr(backport.number)}>{backportWords(backport)}</button>
                {/each}
              </small>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    {#if view.content}
      <RenderedHtml html={view.content.body_html} onlink={(address) => void followLink(address)} />
      {#if view.content.earlier_comments > 0}
        <p class="by">
          {earlierComments(view.content.earlier_comments)}
        </p>
      {/if}
      {#each view.content.comments as comment, position (position)}
        <article>
          <p class="by">{`${comment.author ?? "deleted user"} · ${ago(comment.created_at, now)}`}</p>
          <RenderedHtml html={comment.body_html} onlink={(address) => void followLink(address)} />
        </article>
      {/each}
    {:else}
      <p class="by">Tsugiki hasn't read what this Issue says yet.</p>
    {/if}
  {/if}
</section>

<style>
  /* Stays on screen beside a long list, and scrolls by itself, clear of the status bar. */
  .detail {
    flex: 1;
    min-width: 320px;
    position: sticky;
    top: 0;
    align-self: flex-start;
    max-height: calc(100vh - 48px);
    overflow-y: auto;
  }

  h2 {
    font-size: 16px;
    margin: 0;
  }

  .by {
    font-size: 12px;
    margin: 4px 0;
    opacity: 0.75;
  }

  .link {
    all: unset;
    cursor: pointer;
  }

  .link:hover,
  .link:focus-visible {
    text-decoration: underline;
  }

  ul {
    list-style: none;
    margin: 12px 0;
    padding: 0;
  }

  li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 0;
  }

  article {
    border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    margin-top: 12px;
  }
</style>
