<script lang="ts">
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { IssueView, IssueViewState } from "$lib/state/issue-view.svelte";
  import Conversation from "./Conversation.svelte";
  import { issueIcon, prIcon } from "./octicons";
  import StateIcon from "./StateIcon.svelte";
  import { ago, backportWords, headline, prStateWords } from "./workbench-row";

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

  /** The start of the author line: the Issue's author and how long ago they opened it, with the date and time
   * for the tooltip. A read kept from before Tsugiki asked has neither time, until the next read. */
  const opened = (view: IssueView) => {
    const who = view.author ?? "deleted user";
    const at = view.content?.opened_at;
    if (!at) return { words: who, title: undefined };
    const title = new Date(at).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
    return { words: `${who} opened this ${ago(at, now)}`, title };
  };

  /** The rest of the author line: how many comments, once Tsugiki has read them, and whom it is assigned
   * to. */
  const restOfLine = (view: IssueView) => {
    const parts: string[] = [];
    if (view.content) {
      const count = view.content.comments.length + view.content.earlier_comments;
      parts.push(count === 1 ? "1 comment" : `${String(count)} comments`);
    }
    if (view.assignees.length > 0) parts.push(`assigned to ${view.assignees.join(", ")}`);
    return parts.map((part) => ` · ${part}`).join("");
  };
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
    {@const by = opened(view)}
    <p class="by"><span title={by.title}>{by.words}</span>{restOfLine(view)}</p>

    {#if view.pull_requests.length > 0}
      <ul class="prs">
        {#each view.pull_requests as entry (entry.number)}
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

  li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px 0;
  }
</style>
