<script lang="ts">
  import type { IssueContent } from "$lib/state/issue-view.svelte";
  import RenderedHtml from "./RenderedHtml.svelte";
  import { ago } from "./workbench-row";

  // An Issue's or a PR's description and its recent comments, as GitHub renders them.
  let { content, now, onlink }: { content: IssueContent; now: Date; onlink: (address: string) => void } =
    $props();

  const earlierComments = (count: number) =>
    count === 1 ? "1 earlier comment is on GitHub." : `${String(count)} earlier comments are on GitHub.`;
</script>

<RenderedHtml html={content.body_html} {onlink} />
{#if content.earlier_comments > 0}
  <p class="by">
    {earlierComments(content.earlier_comments)}
  </p>
{/if}
{#each content.comments as comment, position (position)}
  <article>
    <p class="by">{`${comment.author ?? "deleted user"} · ${ago(comment.created_at, now)}`}</p>
    <RenderedHtml html={comment.body_html} {onlink} />
  </article>
{/each}

<style>
  .by {
    font-size: 12px;
    margin: 4px 0;
    opacity: 0.75;
  }

  article {
    border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    margin-top: 12px;
  }
</style>
