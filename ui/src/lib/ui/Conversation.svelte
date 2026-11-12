<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import type { HiddenReason, IssueComment, IssueContent } from "$lib/state/issue-view.svelte";
  import type { PullRequestContent, Review, ReviewState, ThreadLine } from "$lib/state/pr-view.svelte";
  import { REASONS } from "./hidden";
  import RenderedHtml from "./RenderedHtml.svelte";
  import { ago } from "./workbench-row";

  // An Issue's or a PR's description, then its recent comments and a PR's Reviews in time order (#156). A
  // thread's place opens the thread in Files, given `onthread` (#184).
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
    | { kind: "review"; key: string; id: string; at: string; review: Review };

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
    return [...comments, ...reviews].sort((one, other) => Date.parse(one.at) - Date.parse(other.at));
  });
  const earlierReviews = $derived("earlier_reviews" in content ? content.earlier_reviews : 0);

  /** The hidden comments and Reviews the user opened, by their entry's `id`. */
  const opened = new SvelteSet<string>();
  const toggle = (id: string) => {
    if (!opened.delete(id)) opened.add(id);
  };

  const earlierComments = (count: number) =>
    count === 1 ? "1 earlier comment is on GitHub." : `${String(count)} earlier comments are on GitHub.`;

  const DID: Record<ReviewState, string> = {
    APPROVED: "approved",
    CHANGES_REQUESTED: "requested changes",
    COMMENTED: "reviewed",
    DISMISSED: "reviewed",
    PENDING: "has a pending Review",
  };
  const reviewLine = (review: Review) => {
    const at = review.commit === null ? "" : ` at ${review.commit.slice(0, 7)}`;
    const dismissed = review.state === "DISMISSED" ? ", since dismissed" : "";
    const when = ago(review.submitted_at, now);
    return `${review.author ?? "deleted user"} ${DID[review.state]}${at}${dismissed} · ${when}`;
  };

  /** The file, and the line the thread is on, or began on once the code under it changed. */
  const place = ({ path, subject }: ThreadLine) => {
    const line = subject.kind === "line" ? (subject.line ?? subject.original) : null;
    return line === null ? path : `${path}:${String(line)}`;
  };
  const replies = (count: number) => (count === 1 ? "1 reply" : `${String(count)} replies`);
</script>

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

<RenderedHtml html={content.body_html} {onlink} />
{#if content.earlier_comments > 0}
  <p class="by">{earlierComments(content.earlier_comments)}</p>
{/if}
{#if earlierReviews > 0}
  <!-- Some of them may only have replied in threads, which GitHub doesn't show as Reviews. -->
  <p class="by">Older Reviews are on GitHub.</p>
{/if}
{#each entries as entry (entry.key)}
  {#if entry.kind === "comment"}
    <article>
      <p class="by">{`${entry.comment.author ?? "deleted user"} · ${ago(entry.comment.created_at, now)}`}</p>
      {@render body(entry.id, entry.comment.body_html, entry.comment.hidden)}
    </article>
  {:else if entry.review.body_html === "" && entry.review.threads.length === 0}
    <p class="by review">{reviewLine(entry.review)}</p>
  {:else}
    <article>
      <p class="by">{reviewLine(entry.review)}</p>
      {#if entry.review.body_html !== ""}
        {@render body(entry.id, entry.review.body_html, entry.review.hidden)}
      {/if}
      <ul class="threads">
        {#each entry.review.threads as thread, position (position)}
          <li>
            {#if onthread}
              <button class="place" onclick={() => onthread(thread.id)}><code>{place(thread)}</code></button>
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

<style>
  .by {
    font-size: 12px;
    margin: 4px 0;
    color: var(--fg-muted);
  }

  .review {
    border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    margin-top: 12px;
    padding-top: 8px;
  }

  article {
    border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    margin-top: 12px;
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
    padding: 0;
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
