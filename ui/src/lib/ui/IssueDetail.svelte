<script lang="ts">
  import type { Snippet } from "svelte";
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { ReadFailure } from "$lib/state/detail-view.svelte";
  import type { IssueView, IssueViewState } from "$lib/state/issue-view.svelte";
  import type { TreeEntry } from "$lib/state/workbench.svelte";
  import Conversation from "./Conversation.svelte";
  import MarkButton from "./MarkButton.svelte";
  import { issueIcon, prIcon, type Octicon } from "./octicons";
  import StateIcon from "./StateIcon.svelte";
  import StateWords from "./StateWords.svelte";
  import { ago, backportWords, commentCount, headline, openOfAll } from "./workbench-row";

  let {
    repository,
    issue,
    now,
    onopenpr,
    onmark,
    focusable = false,
    header,
  }: {
    repository: string;
    issue: IssueViewState;
    now: Date;
    onopenpr: (number: number) => void;
    /** Mark Seen or Mark Unseen, whichever the header's button offers. */
    onmark: () => void;
    /** Whether the pane takes the focus, as it does while the Issue list is hidden. Otherwise a click in it
     * leaves the focus where it was, so Enter still opens the row under the cursor. */
    focusable?: boolean;
    /** What sits at the top of the pane, such as the history's arrows (#586). */
    header?: Snippet;
  } = $props();

  /** The words in place of what the Issue says before a read lands, and why it hasn't once a read failed. */
  const UNREAD: Record<ReadFailure | "reading", string> = {
    reading: "Tsugiki hasn't read what this Issue says yet.",
    no_token:
      "Tsugiki can't ask GitHub what this Issue says without a token. The status bar says how to sign in.",
    waits_on_github: "What this Issue says comes once GitHub answers.",
    not_on_github: "GitHub doesn't have this Issue, or your token can't see it.",
    other: "Tsugiki couldn't read what this Issue says from GitHub. tsugiki.log says why.",
  };

  /** The words for an Issue Tsugiki doesn't hold, such as one a link named, and why once a read failed. */
  const notHeld = (number: number, why: ReadFailure | undefined) => {
    const issue = `Issue #${String(number)}`;
    const words: Record<ReadFailure | "reading", string> = {
      reading: `Tsugiki doesn't hold ${issue}.`,
      no_token: `Tsugiki doesn't hold ${issue}, and can't ask GitHub for it without a token. The status bar says how to sign in.`,
      waits_on_github: `Tsugiki doesn't hold ${issue}. It comes once GitHub answers.`,
      not_on_github: `GitHub doesn't have ${issue}, or your token can't see it.`,
      other: `Tsugiki doesn't hold ${issue}, and couldn't read it from GitHub. tsugiki.log says why.`,
    };
    return words[why ?? "reading"];
  };

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

  /** The legend under the tree: the words for each icon in it. */
  const LEGEND: [string, Octicon][] = [
    ["Open", prIcon({ state: "OPEN", is_draft: false })],
    ["Draft", prIcon({ state: "OPEN", is_draft: true })],
    ["Merged", prIcon({ state: "MERGED", is_draft: false })],
    ["Closed", prIcon({ state: "CLOSED", is_draft: false })],
  ];

  /** The start of the author line: the Issue's author and how long ago they opened it, with the date and time
   * for the tooltip. A read kept from before Tsugiki asked has neither time, until the next read. */
  const opened = (view: IssueView) => {
    const who = view.author ?? "deleted user";
    const at = view.content?.opened_at;
    if (!at) return { who, words: "", title: undefined };
    const title = new Date(at).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
    return { who, words: ` opened this ${ago(at, now)}`, title };
  };

  /** The rest of the author line: how many comments, once Tsugiki has read them, and whom it is assigned
   * to. */
  const restOfLine = (view: IssueView) => {
    const parts: string[] = [];
    if (view.content) parts.push(commentCount(view.content.comments.length + view.content.earlier_comments));
    if (view.assignees.length > 0) parts.push(`assigned to ${view.assignees.join(", ")}`);
    return parts.map((part) => ` · ${part}`).join("");
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
    <p>{notHeld(issue.number, issue.failedRead)}</p>
  {:else if issue.view}
    {@const view = issue.view}
    {@const icon = issueIcon(view.state)}
    {@const by = opened(view)}
    <div class="page">
      <!-- The icon in the heading names the state to a screen reader already. -->
      <p class="crumb" aria-hidden="true">{icon?.label ?? "Issue"}</p>
      <h2>
        {#if icon}
          <StateIcon {icon} />
        {/if}
        <button class="link" onclick={() => void onGitHub({ kind: "issue", number: view.number })}>
          {headline(view)}
        </button>
      </h2>
      <p class="by"><span title={by.title}><b>{by.who}</b>{by.words}</span>{restOfLine(view)}</p>
      <div class="meta">
        {#if view.labels.length > 0}
          <ul class="labels" aria-label="Labels">
            {#each view.labels as label (label)}
              <li class="chip">{label}</li>
            {/each}
          </ul>
        {/if}
        <MarkButton seen={view.seen} {onmark} />
      </div>

      {#snippet tree(entries: TreeEntry[], name: string)}
        <ul aria-label={name}>
          {#each entries as entry (entry.number)}
            <li>
              <span class="pr">
                <StateIcon icon={prIcon(entry)} />
                <code class="branch" title={entry.target_branch}>{entry.target_branch}</code>
                <button
                  class={["link", "title", { attention: view.needs_attention.includes(entry.number) }]}
                  title={entry.title}
                  onclick={openPr(entry.number)}>{@render numbered(entry)}</button
                >
                <small class="who">{entry.author ?? "deleted user"}</small>
                <StateWords row={entry} />
              </span>
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
      {/snippet}
      {#if view.pull_requests.length > 0 || view.merged_count > 0}
        <!-- The count takes in the merged PRs folded away too. -->
        <h3>Origin PRs <small>{openOfAll([...view.pull_requests, ...view.merged])}</small></h3>
        <div class="prs">
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
        </div>
        <p class="legend">
          <!-- The words name each icon, so a screen reader skips the icons' own names. -->
          {#each LEGEND as [words, icon] (words)}
            <span><span aria-hidden="true"><StateIcon {icon} /></span><small>{words}</small></span>
          {/each}
          <small><b>Bold</b>: Needs attention</small>
        </p>
      {/if}

      {#if view.content}
        <Conversation content={view.content} {now} onlink={(address) => void followLink(address)} />
      {:else}
        <p class="by">{UNREAD[issue.failedRead ?? "reading"]}</p>
      {/if}
    </div>
  {/if}
</section>

<!-- A PR's number in the code font, then its title: one expression each, as Svelte compiles `{a} {b}` with
     branches no test can reach. -->
{#snippet numbered(row: { number: number; title: string })}<code class="number"
    >{`#${String(row.number)}`}</code
  >{` ${row.title}`}{/snippet}

<style>
  /* Takes the window's height and scrolls by itself, as each pane does, and the wheel stops at its end. */
  .detail {
    flex: 1;
    min-width: 320px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* Lines stay short enough to read in a wide window. */
  .page {
    max-width: 900px;
    padding: 10px 16px 24px;
  }

  .crumb {
    margin: 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  h2 {
    margin: 4px 0 2px;
    font-size: 18px;
  }

  .by {
    margin: 0 0 8px;
    font-size: 13px;
    color: var(--fg-muted);
  }

  .by b {
    font-weight: 600;
    color: var(--fg);
  }

  /* The labels' chips, then the Mark Seen button, on one line that wraps in a narrow pane. */
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    margin: 0 0 8px;
  }

  /* The row keeps the space around the button, as margins don't collapse in a flex row. */
  .meta :global(.actions) {
    margin: 0;
  }

  .labels {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  h3 {
    margin: 18px 0 6px;
    font-size: 13px;
  }

  /* The icons in the legend keep the colors of those in the tree, and only its words fade. */
  h3 small,
  .legend small {
    font-size: 11px;
    font-weight: normal;
    color: var(--fg-muted);
  }

  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 6px 0 0;
  }

  .legend span {
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }

  /* The tree: a box with a line between its rows. */
  .prs {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .prs li + li,
  .prs > * + * {
    border-top: 1px solid var(--border);
  }

  /* A row wraps its state words under it once the pane is too narrow for them. */
  .pr {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    gap: 0 8px;
    align-items: center;
    padding: 4px 10px;
    font-size: 13px;
  }

  .pr:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  /* The title's cover takes a click anywhere on the row; the icon sits above it to keep its tooltip. */
  .title::after {
    content: "";
    position: absolute;
    inset: 0;
  }

  .pr > :global(.octicon) {
    position: relative;
    z-index: 1;
  }

  .title:hover:not(:focus-visible) {
    text-decoration: none;
  }

  .branch {
    flex: none;
    min-width: 32px;
    max-width: 8em;
    overflow: hidden;
    font-size: 11px;
    color: var(--fg-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The title takes the room the row has left, and an ellipsis cuts it. A PR that doesn't Need attention
     steps back, and one that does stands out, as in the Issue list. */
  .title {
    flex: 1;
    min-width: 8em;
    overflow: hidden;
    color: var(--fg-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title.attention,
  .backports .attention {
    font-weight: 600;
    color: var(--fg);
  }

  .number {
    font-size: 12px;
    color: var(--accent);
  }

  .who {
    flex: none;
    font-size: 12px;
    color: var(--fg-muted);
  }

  /* An Origin PR's Backports, on one line under it, indented on the panel colour. */
  .backports {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    padding: 2px 10px 4px 34px;
    font-size: 12px;
    color: var(--fg-muted);
    background: var(--bg-chrome);
  }

  .backports span {
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }

  .fold {
    display: block;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--accent);
  }
</style>
