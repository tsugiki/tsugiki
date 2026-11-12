<!-- A PR's Files tab: its diff, or what it waits for, the mirror's clone or the PR's commits. A clone that
     waits for the user to say where the mirror gets its objects asks it here (#311). When the PR has moved
     on since the mirror's copy, the diff before shows while the mirror fetches the newer one, with its
     threads held back and counted (#185). A fetch of the PR's head that fails says why, and so does one that
     leaves GitHub's ref somewhere else or finds none, and "Try again" asks the core once more, which hands
     the head to the mirror again (#233, #213). Ctrl+F opens a search over the diff's rows (#186). -->
<script lang="ts">
  import type { PrFilesState, Unfetched } from "$lib/state/pr-files.svelte";
  import DiffRows from "./DiffRows.svelte";
  import FindBar from "./FindBar.svelte";
  import MirrorQuestion from "./MirrorQuestion.svelte";
  import { cloningWords } from "./synced";

  let { files, now, onlink }: { files: PrFilesState; now: Date; onlink: (address: string) => void } =
    $props();

  /** A commit's short name, as GitHub shows it. */
  const short = (commit: string) => commit.slice(0, 7);
  const waiting = (count: number) =>
    count === 1 ? "1 thread waits for it." : `${String(count)} threads wait for it.`;
  /** git's words for why a fetch failed, in brackets, or nothing if it said none. */
  const because = (words: string) => (words === "" ? "" : ` (${words})`);
  /** The tab's words for the reason the mirror lacks the PR's head, `head` being how the tab names it.
   * GitHub's ref can be behind the head the last sync stored, or past it after a force-push, so these words
   * claim neither. */
  const unfetchedWords = (why: Unfetched, head: string, git: string) => {
    switch (why.kind) {
      case "git":
        return `${git}${because(why.words)}.`;
      case "behind":
        return `GitHub's copy of this PR's branch isn't at ${head}.`;
      case "not_on_github":
        return "GitHub has no copy of this PR's branch.";
    }
  };
</script>

{#if files.error}
  <p role="alert">{`Tsugiki can't show this PR's files: ${files.error}`}</p>
{/if}
{#if files.view?.state === "asking"}
  <MirrorQuestion repository={files.repository} />
{:else if files.view?.state === "cloning"}
  <p class="waiting">{cloningWords(files.cloning) ?? "Cloning the Repository's mirror…"}</p>
{:else if files.view?.state === "fetching"}
  {#if files.unfetched !== undefined}
    <p class="waiting" role="alert">
      {unfetchedWords(
        files.unfetched,
        "the commit Tsugiki last synced",
        "Tsugiki couldn't fetch the PR's commits",
      )}
      <button onclick={() => void files.ask()}>Try again</button>
    </p>
  {:else}
    <p class="waiting">Fetching the PR's commits…</p>
  {/if}
{:else if files.view?.state === "ready"}
  {#if files.heldBack !== undefined && files.unfetched !== undefined}
    {@const [shown, newer] = [short(files.view.head), short(files.heldBack.newer)]}
    <p class="waiting" role="alert">
      {`Showing ${shown} · ${unfetchedWords(files.unfetched, newer, `Tsugiki couldn't fetch the newer ${newer}`)}`}
      {#if files.heldBack.count > 0}{waiting(files.heldBack.count)}{/if}
      <button onclick={() => void files.ask()}>Try again</button>
    </p>
  {:else if files.heldBack !== undefined}
    <p class="waiting" role="status">
      {`Showing ${short(files.view.head)} · fetching the newer ${short(files.heldBack.newer)}.`}
      {#if files.heldBack.count > 0}{waiting(files.heldBack.count)}{/if}
    </p>
  {/if}
  {#if files.searching}
    <FindBar {files} />
  {/if}
  <div class="diff">
    <DiffRows diff={files} files={files.view.files} news={files.view.news} {now} {onlink} />
  </div>
{/if}

<style>
  .waiting {
    font-size: 12px;
    color: var(--fg-muted);
  }

  /* The diff fills what the tab leaves, and scrolls by itself. */
  .diff {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
