<script lang="ts">
  import type { Snippet } from "svelte";
  import { counted } from "$lib/state/counted";
  import type { ReadFailure } from "$lib/state/detail-view.svelte";
  import type { IssueView, IssueViewState } from "$lib/state/issue-view.svelte";
  import { LinkOpener } from "$lib/state/link-opener.svelte";
  import type { RepositoryState } from "$lib/state/repository-state.svelte";
  import { isMuted, treeRows, type TreeEntry } from "$lib/state/workbench.svelte";
  import Conversation from "./Conversation.svelte";
  import HideButtons from "./HideButtons.svelte";
  import MarkButton from "./MarkButton.svelte";
  import { issueIcon, prIcon, type Octicon } from "./octicons";
  import PersonLink from "./PersonLink.svelte";
  import ReadOnlyNote from "./ReadOnlyNote.svelte";
  import StateIcon from "./StateIcon.svelte";
  import StateWords from "./StateWords.svelte";
  import { stampOf } from "./time";
  import { ago, backportWords, commentCount, foldedTip, headline, updatedTip } from "./workbench-row";

  let {
    repository,
    issue,
    now,
    onopenpr,
    onmark,
    onarchive,
    onmute,
    onmuteauthor,
    onpin,
    onperson = () => {},
    focusable = false,
    header,
    repositoryState,
  }: {
    repository: string;
    issue: IssueViewState;
    now: Date;
    onopenpr: (number: number) => void;
    /** Mark Seen or Mark Unseen, whichever the header's button offers. */
    onmark: () => void;
    /** Archive, or Unarchive on an Archived Issue, as the header's button offers for the view on show
     * (#490). */
    onarchive: (view: IssueView) => void;
    /** Mute, or Unmute on a Muted Issue, as the header's button offers for the view on show (#492). */
    onmute: (view: IssueView) => void;
    /** Mute author on the Issue's author, or Unmute author on a Muted author, as the header's button offers
     * for the view on show (#494). */
    onmuteauthor: (view: IssueView) => void;
    /** Pin, or Unpin on a Pinned issue, as the header's ★ offers for the view on show (#488). */
    onpin: (view: IssueView) => void;
    /** Opens the person Filter for a clicked name (#472). */
    onperson?: (login: string) => void;
    /** Whether the pane takes the focus itself: while the Issue list is hidden, and while the list shows
     * after a click in its controls or a press of F6. Otherwise a click on its background drops the focus to
     * the page, which the list's keys take, so Enter still opens the row under the cursor. */
    focusable?: boolean;
    /** What sits at the top of the pane, such as the history's arrows (#586). */
    header?: Snippet;
    /** The Repository's state, which says atop the view why Tsugiki can't write there (#398). */
    repositoryState?: RepositoryState | undefined;
  } = $props();

  /** The words in place of what the Issue says before a read lands, and why it hasn't once a read failed. */
  const UNREAD: Record<ReadFailure | "reading", string> = {
    reading: "Tsugiki hasn't read what this Issue says yet.",
    no_token: "Tsugiki can't ask GitHub what this Issue says without a token. The status bar says why.",
    waits_on_github: "What this Issue says comes once GitHub answers.",
    not_on_github: "GitHub doesn't have this Issue, or your token can't see it.",
    other: "Tsugiki couldn't read what this Issue says from GitHub. tsugiki.log says why.",
  };

  /** The words for an Issue Tsugiki doesn't hold, such as one a link named, and why once a read failed. */
  const notHeld = (number: number, why: ReadFailure | undefined) => {
    const issue = `Issue #${String(number)}`;
    const words: Record<ReadFailure | "reading", string> = {
      reading: `Tsugiki doesn't hold ${issue}.`,
      no_token: `Tsugiki doesn't hold ${issue}, and can't ask GitHub for it without a token. The status bar says why.`,
      waits_on_github: `Tsugiki doesn't hold ${issue}. It comes once GitHub answers.`,
      not_on_github: `GitHub doesn't have ${issue}, or your token can't see it.`,
      other: `Tsugiki doesn't hold ${issue}, and couldn't read it from GitHub. tsugiki.log says why.`,
    };
    return words[why ?? "reading"];
  };

  const links = new LinkOpener();
  const openPr = (number: number) => () => {
    onopenpr(number);
  };

  /** The states the row above the tree counts, in its order, each by the icon its PRs show. */
  const STATES: [string, Octicon][] = [
    ["Open", prIcon({ state: "OPEN", is_draft: false })],
    ["Draft", prIcon({ state: "OPEN", is_draft: true })],
    ["Merged", prIcon({ state: "MERGED", is_draft: false })],
    ["Closed", prIcon({ state: "CLOSED", is_draft: false })],
  ];

  /** The pause between an entry's word and its count, which only a screen reader hears. Svelte would trim
   * the space if the markup held it. */
  const PAUSE = ", ";

  /** How many PRs show each state's icon, of the shown ones and those folded away, Backports included. A
   * draft counts as Draft, as its icon says, and not as Open. `prIcon` hands out one shared icon per state,
   * so the icons compare as objects. */
  const stateCounts = (view: IssueView) => {
    const icons = treeRows([...view.pull_requests, ...view.merged]).map(prIcon);
    return STATES.map(([words, icon]) => ({
      words,
      icon,
      count: icons.filter((shown) => shown === icon).length,
    }));
  };

  /** The start of the author line: the Issue's author and how long ago they opened it, with the date and time
   * for the tooltip. A read kept from before Tsugiki asked has neither time, until the next read. */
  const opened = (view: IssueView) => {
    const who = view.author ?? "deleted user";
    const at = view.content?.opened_at;
    if (!at) return { who, words: "", title: undefined };
    return { who, words: ` opened this ${ago(at, now)}`, title: stampOf(at) };
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
    `${shown ? "Hide" : "Show"} ${counted(count, "merged PR", "merged PRs")}`;
</script>

<!-- The pane mounts afresh each time the view shows, so it goes back to where the view says it was. -->
<section
  class="detail-pane"
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
  <ReadOnlyNote {repositoryState} {repository} />
  {#if issue.error}
    <p role="alert">{`Tsugiki can't show this Issue: ${issue.error}`}</p>
  {/if}
  {#if links.problem}
    <p role="alert">{links.problem}</p>
  {/if}
  {#if issue.view === null}
    <p>{notHeld(issue.number, issue.failedRead)}</p>
  {:else if issue.view}
    {@const view = issue.view}
    {@const icon = issueIcon(view.state)}
    {@const by = opened(view)}
    {@const muted = isMuted(view.hidden)}
    <div class="page">
      <!-- The icon in the heading names the state to a screen reader already. -->
      <p class="crumb" aria-hidden="true">{icon?.label ?? "Issue"}</p>
      <h2 class="title-line">
        {#if icon}
          <span class="state-icon"><StateIcon {icon} /></span>
        {/if}
        <button
          class="link"
          onclick={() => void links.openOnGitHub(repository, { kind: "issue", number: view.number })}
        >
          {headline(view)}
        </button>
      </h2>
      <p class="byline">
        <span title={by.title}
          ><b
            >{#if view.author}<PersonLink login={view.author} onopen={onperson} />{:else}{by.who}{/if}</b
          >{by.words}</span
        >{restOfLine(view)}
      </p>
      <div class="meta">
        {#if view.labels.length > 0}
          <ul class="labels" role="list" aria-label="Labels">
            {#each view.labels as label (label)}
              <li class="chip">{label}</li>
            {/each}
          </ul>
        {/if}
        <!-- A Muted Issue offers Unmute and Mute author (#492), and a Muted author's Issue only Unmute author
             (#494). Mute author is off on the user's own Issue. -->
        <MarkButton seen={view.seen} {onmark} marks={!muted}>
          <button
            onclick={() => {
              onpin(view);
            }}>{view.pinned ? "★ Unpin" : "☆ Pin"}</button
          >
          <HideButtons {view} {onarchive} {onmute} {onmuteauthor} />
        </MarkButton>
      </div>

      {#snippet tree(entries: TreeEntry[], name: string)}
        <ul role="list" aria-label={name}>
          {#each entries as entry (entry.number)}
            <li>
              <span class="pr" title={updatedTip(entry)}>
                <StateIcon icon={prIcon(entry)} />
                <code class="branch" title={entry.target_branch}>{entry.target_branch}</code>
                <button
                  class={["link", "title", { attention: view.needs_attention.includes(entry.number) }]}
                  title={entry.title}
                  onclick={openPr(entry.number)}>{@render numbered(entry)}</button
                >
                <small class="who"
                  >{#if entry.author}<PersonLink login={entry.author} onopen={onperson} />{:else}deleted user{/if}</small
                >
                <StateWords row={entry} />
              </span>
              {#if entry.kind === "pr" && entry.backports.length > 0}
                <small class="backports">
                  Backports:
                  {#each entry.backports as backport (backport.number)}
                    <span title={updatedTip(backport)}>
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
        <!-- Derived once per view, as `{@const}` is, where a `$derived` would need a branch for no view that
             the template never reads. -->
        {@const counts = stateCounts(view)}
        <h3>Origin PRs</h3>
        <!-- Each entry reads its words, then a comma only a screen reader hears, then its count. The words
             name each icon, so a screen reader skips the icons' own names, and "Bold", which names how the
             tree shows a PR. -->
        <ul class="counts" role="list" aria-label="PR counts">
          {#each counts as { words, icon, count } (words)}
            <li class={{ none: count === 0 }}>
              <span aria-hidden="true"><StateIcon {icon} /></span><span>{words}</span>{@render tally(count)}
            </li>
          {/each}
          <li class={{ none: view.needs_attention.length === 0 }}>
            <span><span aria-hidden="true"><b>Bold</b>:</span> Needs attention</span>{@render tally(
              view.needs_attention.length,
            )}
          </li>
        </ul>
        <div class="prs">
          {#if view.pull_requests.length > 0}
            {@render tree(view.pull_requests, "PRs")}
          {/if}
          {#if view.merged_count > 0}
            <button
              class="link fold"
              aria-expanded={issue.mergedShown}
              title={foldedTip(view.merged)}
              onclick={() => issue.toggleMerged()}
            >
              {mergedWords(issue.mergedShown, view.merged_count)}
            </button>
            {#if issue.mergedShown}
              {@render tree(view.merged, "Merged PRs")}
            {/if}
          {/if}
        </div>
      {/if}

      {#if view.content}
        <Conversation
          content={view.content}
          {now}
          {onperson}
          onlink={(address) => void links.openLink(address)}
        />
      {:else}
        <p class="byline">{UNREAD[issue.failedRead ?? "reading"]}</p>
      {/if}
    </div>
  {/if}
</section>

<!-- A PR's number in the code font, then its title: one expression each, as Svelte compiles `{a} {b}` with
     branches no test can reach. -->
{#snippet numbered(row: { number: number; title: string })}<code class="number"
    >{`#${String(row.number)}`}</code
  >{` ${row.title}`}{/snippet}

<!-- A count in the row above the tree, after a comma only a screen reader hears: "Open, 1". -->
{#snippet tally(count: number)}<span class="for-screen-readers">{PAUSE}</span><span class="count"
    >{count}</span
  >{/snippet}

<style>
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

  /* The row above the tree: each state's icon in the colours of the tree's, its word, then its count, muted
     as on GitHub's tabs. A state no PR is in fades whole, and keeps its place. */
  .counts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    margin: 0 0 6px;
    overflow: hidden;
    font-size: 12px;
  }

  /* A muted dot in the gap before each entry, which the empty text after the slash keeps from a screen
     reader. An entry that starts a line has its dot left of the row, where the row's edge cuts it off. */
  .counts li::before {
    content: "·" / "";
    position: absolute;
    left: -8px;
    color: var(--fg-muted);
  }

  .counts li {
    position: relative;
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }

  .count,
  .counts .none {
    color: var(--fg-muted);
  }

  .counts .none :global(.octicon) {
    color: var(--state-muted);
  }

  /* The tree: a box with a line between its rows. */
  .prs {
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  /* WebKit drops the list role of a list without bullets, so each list here writes out `role="list"`. */
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
