<script lang="ts">
  import type { Snippet } from "svelte";
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { IssueViewState } from "$lib/state/issue-view.svelte";
  import type { TreeEntry } from "$lib/state/workbench.svelte";
  import Conversation from "./Conversation.svelte";
  import { issueIcon, prIcon } from "./octicons";
  import StateIcon from "./StateIcon.svelte";
  import { backportWords, headline, prStateWords } from "./workbench-row";

  let {
    repository,
    issue,
    now,
    onopenpr,
    focusable = false,
    header,
  }: {
    repository: string;
    issue: IssueViewState;
    now: Date;
    onopenpr: (number: number) => void;
    /** Whether the pane takes the focus, as it does while the Issue list is hidden. Otherwise a click in it
     * leaves the focus where it was, so Enter still opens the row under the cursor. */
    focusable?: boolean;
    /** What sits at the top of the pane, such as the history's arrows (#586). */
    header?: Snippet;
  } = $props();

  let browserProblem = $state<string>();

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(repository, item);
  };
  const followLink = async (address: string) => {
    browserProblem = await openLink(address);
  };
  const openPr = (number: number) => () => {
    onopenpr(number);
  };
  /** The line the fully merged PRs fold into, which says what a click does (#588). */
  const mergedWords = (shown: boolean, count: number) =>
    `${shown ? "Hide" : "Show"} ${String(count)} merged ${count === 1 ? "PR" : "PRs"}`;
</script>

<!-- The pane mounts afresh each time the view shows, so it goes back to where the view says it was. -->
<section
  class="detail"
  aria-label="Issue view"
  tabindex={focusable ? -1 : undefined}
  data-pane="detail"
  {@attach (pane) => {
    pane.scrollTop = issue.scrollTop;
  }}
  onscroll={(e) => {
    issue.scrollTop = e.currentTarget.scrollTop;
  }}
>
  {@render header?.()}
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
    {@const icon = issueIcon(view.state)}
    <h2>
      {#if icon}
        <StateIcon {icon} />
      {/if}
      <button class="link" onclick={() => void onGitHub({ kind: "issue", number: view.number })}>
        {headline(view)}
      </button>
    </h2>
    <p class="by">{view.author ?? "deleted user"}</p>

    {#snippet tree(entries: TreeEntry[], label: string)}
      <ul aria-label={label}>
        {#each entries as entry (entry.number)}
          <li>
            <span>
              <StateIcon icon={prIcon(entry)} />
              <button class="link" onclick={openPr(entry.number)}>{headline(entry)}</button>
            </span>
            <small>{prStateWords(entry)}</small>
            {#if entry.kind === "pr" && entry.backports.length > 0}
              <small class="backports">
                Backports:
                {#each entry.backports as backport (backport.number)}
                  <span>
                    <StateIcon icon={prIcon(backport)} />
                    <button class="link" onclick={openPr(backport.number)}>{backportWords(backport)}</button>
                  </span>
                {/each}
              </small>
            {/if}
          </li>
        {/each}
      </ul>
    {/snippet}
    {#if view.pull_requests.length > 0}
      {@render tree(view.pull_requests, "PRs")}
    {/if}
    {#if view.merged_count > 0}
      <button class="link fold" aria-expanded={issue.mergedShown} onclick={() => issue.toggleMerged()}>
        {mergedWords(issue.mergedShown, view.merged_count)}
      </button>
      {#if issue.mergedShown}
        {@render tree(view.merged, "Merged PRs")}
      {/if}
    {/if}

    {#if view.content}
      <Conversation content={view.content} {now} onlink={(address) => void followLink(address)} />
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

  .fold {
    font-size: 12px;
    opacity: 0.75;
  }

  li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 0;
  }
</style>
