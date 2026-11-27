<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { counted } from "$lib/state/counted";
  import type {
    CommitReference,
    HiddenReason,
    IssueComment,
    IssueContent,
    IssueEvent,
    Reference,
  } from "$lib/state/issue-view.svelte";
  import type { PullRequestContent, Review, ReviewState, ThreadLine } from "$lib/state/pr-view.svelte";
  import { REASONS } from "./hidden";
  import { followLink, linkTitle } from "./link-tooltip";
  import RenderedHtml from "./RenderedHtml.svelte";
  import { stampOf } from "./time";
  import { ago } from "./workbench-row";

  // An Issue's or a PR's description, then its recent comments and events and a PR's Reviews in time order
  // (#156, #1067). A thread's place opens the thread in Files, given `onthread` (#184).
  let {
    content,
    now,
    onlink,
    onthread,
  }: {
    content: IssueContent | PullRequestContent;
    now: Date;
    onlink: (address: string) => void;
    onthread?: (id: string) => void;
  } = $props();

  // `key` is unique on this screen. `id` names the comment or Review through a read that brings more, which
  // moves each one's place.
  type Entry =
    | { kind: "comment"; key: string; id: string; at: string; comment: IssueComment }
    | { kind: "review"; key: string; id: string; at: string; review: Review }
    | { kind: "event"; key: string; at: string; event: IssueEvent };

  const entries = $derived.by((): Entry[] => {
    const comments = content.comments.map((comment, position): Entry => ({
      kind: "comment",
      key: `c${String(position)}`,
      id: `comment ${comment.author ?? ""} ${comment.created_at}`,
      at: comment.created_at,
      comment,
    }));
    const reviews = ("reviews" in content ? content.reviews : []).map((review, position): Entry => ({
      kind: "review",
      key: `r${String(position)}`,
      id: `review ${review.author ?? ""} ${review.submitted_at}`,
      at: review.submitted_at,
      review,
    }));
    const events = content.events.map((event, position): Entry => ({
      kind: "event",
      key: `e${String(position)}`,
      at: event.created_at,
      event,
    }));
    return [...comments, ...reviews, ...events].sort(
      (one, other) => Date.parse(one.at) - Date.parse(other.at),
    );
  });
  const earlierReviews = $derived("earlier_reviews" in content ? content.earlier_reviews : 0);

  /** The hidden comments and Reviews the user opened, by their entry's `id`. */
  const opened = new SvelteSet<string>();
  const toggle = (id: string) => {
    if (!opened.delete(id)) opened.add(id);
  };

  /** A count of what only GitHub holds yet, in words: "1 earlier comment is on GitHub." */
  const earlierWords = (count: number, one: string, many: string) =>
    `${counted(count, one, many)} ${count === 1 ? "is" : "are"} on GitHub.`;
  const earlierComments = (count: number) => earlierWords(count, "earlier comment", "earlier comments");
  const earlierEvents = (count: number) => earlierWords(count, "earlier event", "earlier events");

  const DID: Record<ReviewState, string> = {
    APPROVED: "approved",
    CHANGES_REQUESTED: "requested changes",
    COMMENTED: "reviewed",
    DISMISSED: "reviewed",
    PENDING: "has a pending Review",
  };
  /** What follows a Review's state chip: the commit it reviewed, whether it was dismissed, and when. */
  const reviewRest = (review: Review) => {
    const at = review.commit === null ? "" : ` at ${review.commit.slice(0, 7)}`;
    const dismissed = review.state === "DISMISSED" ? ", since dismissed" : "";
    return `${at}${dismissed} · ${ago(review.submitted_at, now)}`;
  };

  /** The file, and the line the thread is on, or began on once the code under it changed. */
  const place = ({ path, subject }: ThreadLine) => {
    const line = subject.kind === "line" ? (subject.line ?? subject.original) : null;
    return line === null ? path : `${path}:${String(line)}`;
  };
  const replies = (count: number) => counted(count, "reply", "replies");
</script>

<!-- eslint-disable svelte/no-navigation-without-resolve -- The links open in the system browser through
     followLink, and none moves the window. -->

{#snippet body(id: string, html: string, hidden: HiddenReason | null)}
  {#if hidden === null}
    <RenderedHtml {html} {onlink} />
  {:else}
    <button class="link fold" aria-expanded={opened.has(id)} onclick={() => toggle(id)}
      >{REASONS[hidden]}</button
    >
    {#if opened.has(id)}
      <RenderedHtml {html} {onlink} />
    {/if}
  {/if}
{/snippet}

<!-- A Review's author, its state as a chip, then the rest: the words read as one line, as before. -->
{#snippet reviewLine(review: Review)}<b>{review.author ?? "deleted user"}</b>
  <span class={["chip", review.state.toLowerCase()]}>{DID[review.state]}</span><span
    class="muted"
    title={stampOf(review.submitted_at)}>{reviewRest(review)}</span
  >{/snippet}

<!-- A PR or an Issue an event names, as the number that opens it and the title it has. -->
{#snippet reference({ repository, number, title, url }: Reference)}
  <a
    href={url}
    title={linkTitle(url)}
    onclick={(e) => followLink(e, onlink)}
    onauxclick={(e) => {
      // A middle click follows it too, as any link in a description does.
      if (e.button === 1) followLink(e, onlink);
    }}>{`${repository}#${String(number)}`}</a
  >{` ${title}`}
{/snippet}

<!-- A commit an event names: its short SHA, which opens it, then the first line of its message. -->
{#snippet commit({ sha, message, url }: CommitReference)}
  <a
    href={url}
    title={linkTitle(url)}
    onclick={(e) => followLink(e, onlink)}
    onauxclick={(e) => {
      if (e.button === 1) followLink(e, onlink);
    }}>{sha}</a
  >{` ${message}`}
{/snippet}

<!-- One event between the comments, in one line: who, what, and how long ago, with the date in its
     tooltip. A close says whether it was completed or not planned, a rename gives the title it had, a label
     event names the label, and an assignment the assignee (#1067). -->
{#snippet eventLine(event: IssueEvent)}
  <b>{event.actor ?? "deleted user"}</b>
  {#if event.kind === "cross-referenced"}
    mentioned this in {@render reference(event.reference)}
  {:else if event.kind === "referenced"}
    added a commit that references this: {@render commit(event.commit)}
  {:else if event.kind === "connected"}
    connected this to {@render reference(event.reference)}
  {:else if event.kind === "disconnected"}
    disconnected this from {@render reference(event.reference)}
  {:else if event.kind === "labeled"}
    added the {event.label} label
  {:else if event.kind === "unlabeled"}
    removed the {event.label} label
  {:else if event.kind === "assigned"}
    assigned {event.assignee ?? "deleted user"}
  {:else if event.kind === "unassigned"}
    unassigned {event.assignee ?? "deleted user"}
  {:else if event.kind === "closed"}
    {event.completed ? "closed this as completed" : "closed this as not planned"}
  {:else if event.kind === "reopened"}
    reopened this
  {:else}
    renamed this from "{event.previous_title}"
  {/if}
  <span class="muted" title={stampOf(event.created_at)}>{` · ${ago(event.created_at, now)}`}</span>
{/snippet}

<div class="conversation">
  <h3>Description</h3>
  <div class="post">
    {#if content.body_html === ""}
      <p class="muted">No description.</p>
    {:else}
      <RenderedHtml html={content.body_html} {onlink} />
    {/if}
  </div>
  {#if content.earlier_comments > 0}
    <p class="by">{earlierComments(content.earlier_comments)}</p>
  {/if}
  {#if content.earlier_events > 0}
    <p class="by">{earlierEvents(content.earlier_events)}</p>
  {/if}
  {#if earlierReviews > 0}
    <!-- Some of them may only have replied in threads, which GitHub doesn't show as Reviews. -->
    <p class="by">Older Reviews are on GitHub.</p>
  {/if}
  {#each entries as entry (entry.key)}
    {#if entry.kind === "comment"}
      <article>
        <header>
          <b>{entry.comment.author ?? "deleted user"}</b><span
            class="muted"
            title={stampOf(entry.comment.created_at)}>{` · ${ago(entry.comment.created_at, now)}`}</span
          >
        </header>
        <div class="body">{@render body(entry.id, entry.comment.body_html, entry.comment.hidden)}</div>
      </article>
    {:else if entry.kind === "event"}
      <p class="event">{@render eventLine(entry.event)}</p>
    {:else if entry.review.body_html === "" && entry.review.threads.length === 0}
      <p class="review">{@render reviewLine(entry.review)}</p>
    {:else}
      <article>
        <header>{@render reviewLine(entry.review)}</header>
        {#if entry.review.body_html !== ""}
          <div class="body">{@render body(entry.id, entry.review.body_html, entry.review.hidden)}</div>
        {/if}
        <ul class="threads">
          {#each entry.review.threads as thread, position (position)}
            <li>
              {#if onthread}
                <button class="place" onclick={() => onthread(thread.id)}><code>{place(thread)}</code></button
                >
              {:else}
                <code class="place">{place(thread)}</code>
              {/if}
              {#if thread.opening.kind === "words"}
                <span>{thread.opening.words}</span>
              {:else}
                <span class="by">{REASONS[thread.opening.reason]}</span>
              {/if}
              {#if thread.replies > 0}
                <span class="by">{replies(thread.replies)}</span>
              {/if}
              {#if thread.resolution === "resolved"}
                <span class="chip">Resolved</span>
              {/if}
              {#if thread.anchor === "outdated"}
                <span class="chip">Outdated</span>
              {/if}
            </li>
          {/each}
        </ul>
      </article>
    {/if}
  {/each}
</div>

<style>
  /* Lines stay short enough to read in a wide window. */
  .conversation {
    max-width: 860px;
  }

  h3 {
    margin: 18px 0 6px;
    font-size: 13px;
  }

  .by {
    font-size: 12px;
    margin: 4px 0;
    color: var(--fg-muted);
  }

  .muted {
    color: var(--fg-muted);
  }

  /* The description, each comment and each Review with a body: a box with a header on the panel colour. */
  .post,
  article {
    margin: 10px 0;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .post,
  .body {
    padding: 4px 12px;
  }

  header {
    padding: 6px 12px;
    font-size: 13px;
    background: var(--bg-chrome);
    border-bottom: 1px solid var(--border);
    border-radius: 6px 6px 0 0;
  }

  /* A Review with nothing to show but its state. */
  .review {
    margin: 10px 0;
    padding: 0 12px;
    font-size: 13px;
  }

  /* One event between the comments, on a line of its own. */
  .event {
    margin: 6px 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .event b {
    font-weight: 600;
    color: var(--fg);
  }

  .event a {
    color: var(--accent);
  }

  .chip.approved {
    color: var(--state-open);
  }

  .chip.changes_requested {
    color: var(--danger);
  }

  .fold {
    display: block;
    font-size: 12px;
    margin: 4px 0;
    color: var(--fg-muted);
  }

  .threads {
    list-style: none;
    margin: 4px 0 8px;
    padding: 0 12px;
  }

  .threads li {
    display: flex;
    gap: 8px;
    font-size: 12px;
    overflow: hidden;
    white-space: nowrap;
  }

  .threads li > span {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .place,
  .chip {
    flex: none;
  }

  /* A thread's place opens it in Files, and looks like a link. */
  button.place {
    all: unset;
    flex: none;
    cursor: pointer;
    text-decoration: underline;
  }

  button.place:focus-visible {
    outline: 1px solid currentColor;
  }
</style>
