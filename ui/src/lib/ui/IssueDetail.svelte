<script lang="ts">
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { IssueViewState } from "$lib/state/issue-view.svelte";
  import Conversation from "./Conversation.svelte";
  import { issueIcon, prIcon, type Octicon } from "./octicons";
  import StateIcon from "./StateIcon.svelte";
  import { backportWords, headline, openOfAll, prStateWords } from "./workbench-row";

  let {
    repository,
    issue,
    now,
    onopenpr,
    focusable = false,
  }: {
    repository: string;
    issue: IssueViewState;
    now: Date;
    onopenpr: (number: number) => void;
    /** Whether the pane takes the focus, as it does while the Issue list is hidden. Otherwise a click in it
     * leaves the focus where it was, so Enter still opens the row under the cursor. */
    focusable?: boolean;
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

  /** What each icon in the tree says, under it. */
  const LEGEND: [string, Octicon][] = [
    ["Open", prIcon({ state: "OPEN", is_draft: false })],
    ["Draft", prIcon({ state: "OPEN", is_draft: true })],
    ["Merged", prIcon({ state: "MERGED", is_draft: false })],
    ["Closed", prIcon({ state: "CLOSED", is_draft: false })],
  ];
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

    {#if view.pull_requests.length > 0}
      <h3>Origin PRs <small>{openOfAll(view.pull_requests)}</small></h3>
      <ul class="prs" aria-label="Origin PRs">
        {#each view.pull_requests as entry (entry.number)}
          <li>
            <span>
              <StateIcon icon={prIcon(entry)} />
              <code>{entry.target_branch}</code>
              <button
                class={["link", { attention: view.needs_attention.includes(entry.number) }]}
                onclick={openPr(entry.number)}>{headline(entry)}</button
              >
              <small>{entry.author ?? "deleted user"}</small>
            </span>
            <small>{prStateWords(entry)}</small>
            {#if entry.kind === "pr" && entry.backports.length > 0}
              <small class="backports">
                Backports:
                {#each entry.backports as backport (backport.number)}
                  <span>
                    <StateIcon icon={prIcon(backport)} />
                    <button
                      class={["link", { attention: view.needs_attention.includes(backport.number) }]}
                      onclick={openPr(backport.number)}>{backportWords(backport)}</button
                    >
                  </span>
                {/each}
              </small>
            {/if}
          </li>
        {/each}
      </ul>
      <p class="legend">
        {#each LEGEND as [words, icon] (words)}
          <span><StateIcon {icon} />{words}</span>
        {/each}
        <span><b>Bold</b>: Needs attention</span>
      </p>
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

  /* As an item that Needs attention stands out in the Issue list. */
  .attention {
    font-weight: 600;
  }

  h3 {
    font-size: 13px;
    margin: 12px 0 0;
  }

  h3 small,
  .legend {
    font-size: 11px;
    font-weight: normal;
    opacity: 0.75;
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 0 0 12px;
  }

  .legend span {
    display: inline-flex;
    gap: 4px;
    align-items: center;
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
</style>
