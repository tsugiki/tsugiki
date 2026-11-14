<!-- A PR's Files tab: its diff, or what it waits for, the mirror's clone or the PR's commits. A clone that
     waits for the user to say where the mirror gets its objects asks it here (#311). When the PR has moved
     on since the mirror's copy, the diff before shows while the mirror fetches the newer one, with its
     threads held back and counted (#185). A fetch of the PR's head that fails says why, and so does one that
     leaves GitHub's ref somewhere else or finds none, and "Try again" asks the core once more, which hands
     the head to the mirror again (#233, #213). While GitHub can't be reached, the mirror holds back git, and
     the tab says the PR's commits or the mirror's clone come once GitHub answers (#390). Ctrl+F opens a
     search over the diff's rows (#186). The header's toggle switches between all the files and one at a
     time, and one at a time a bar says which file shows, "‹ 12 of 48 ›" and its path (#605). Beside the
     toggle, Go to definition says it searches, or for a moment that it found nothing (#444). A definition in
     a file the PR doesn't change opens the file view over the tab (#453). -->
<script lang="ts">
  import type { Page, PrFilesState, Unfetched } from "$lib/state/pr-files.svelte";
  import DiffRows from "./DiffRows.svelte";
  import FileView from "./FileView.svelte";
  import FindBar from "./FindBar.svelte";
  import { chordLabel, currentOs, keyLabel } from "./keys";
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
  const os = currentOs();
  /** The bar's words for `page`: where its file comes among the files, and its path, or what it shows. */
  const pageWords = (page: Page) => {
    const counted = files.pages.counted(page);
    if (counted !== undefined)
      return { count: `${String(counted.at)} of ${String(counted.count)}`, path: counted.path };
    return { count: "", path: page.kind === "news" ? "NEWS entries" : "On files no longer in the diff" };
  };
</script>

<!-- The bar's button for the file before or after the one on show. -->
{#snippet stepper(page: Page, by: 1 | -1)}
  {@const [name, key, arrow] =
    by === 1
      ? (["Next file", "alt+pagedown", "›"] as const)
      : (["Previous file", "alt+pageup", "‹"] as const)}
  <button
    type="button"
    aria-label={name}
    title={`${name} (${keyLabel(key, os)})`}
    disabled={files.pages.step(page, by) === undefined}
    onclick={() => {
      files.nextFile(by);
    }}>{arrow}</button
  >
{/snippet}

<!-- The file view covers the tab, which takes no focus and no keys under it, and leaves the tree a screen
     reader reads. -->
<div class="covered" inert={files.fileView !== undefined}>
  {#if files.error}
    <p role="alert">{`Tsugiki can't show this PR's files: ${files.error}`}</p>
  {/if}
  {#if files.view?.state === "asking"}
    <MirrorQuestion repository={files.repository} />
  {:else if files.view?.state === "cloning"}
    {#if files.mirror?.state === "offline"}
      <p class="waiting" role="status">{`Cloning ${files.repository} starts once GitHub answers.`}</p>
    {:else}
      <p class="waiting">{cloningWords(files.cloning) ?? "Cloning the Repository's mirror…"}</p>
    {/if}
  {:else if files.view?.state === "fetching"}
    {#if files.waitsForNetwork}
      <p class="waiting" role="status">The PR's commits come once GitHub answers.</p>
    {:else if files.unfetched !== undefined}
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
    {#if files.heldBack !== undefined && files.waitsForNetwork}
      <p class="waiting" role="status">
        {`Showing ${short(files.view.head)} · the newer ${short(files.heldBack.newer)} comes once GitHub answers.`}
        {#if files.heldBack.count > 0}{waiting(files.heldBack.count)}{/if}
      </p>
    {:else if files.heldBack !== undefined && files.unfetched !== undefined}
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
    <div class="header">
      {#if files.shown !== undefined}
        {@const shown = files.shown}
        {@const words = pageWords(shown)}
        <nav class="bar" aria-label="File on show">
          {@render stepper(shown, -1)}<span class="count">{words.count}</span>{@render stepper(shown, 1)}
          <span class="path">{words.path}</span>
        </nav>
      {/if}
      {#if files.notice !== undefined}
        <span class="notice" role="status">{files.notice}</span>
      {/if}
      <button
        type="button"
        class="mode"
        aria-pressed={files.mode === "one"}
        title={`One file at a time (${chordLabel("ctrl+k o", os)})`}
        onclick={() => {
          files.toggleMode();
        }}>One file at a time</button
      >
    </div>
    {#if files.searching}
      <FindBar {files} />
    {/if}
    <div class="diff">
      <DiffRows diff={files} files={files.view.files} news={files.view.news} {now} {onlink} />
    </div>
  {/if}
</div>
{#if files.fileView}
  <FileView
    view={files.fileView}
    number={files.number}
    onback={() => {
      files.closeFileView();
    }}
  />
{/if}

<style>
  /* Lays its children out in the tab as if it weren't there. */
  .covered {
    display: contents;
  }

  .waiting {
    font-size: 12px;
    color: var(--fg-muted);
  }

  /* One line over the diff: the bar on the left, the toggle on the right. */
  .header {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 2px 8px;
    font-size: 12px;
  }

  .bar {
    display: flex;
    flex: 1;
    gap: 4px;
    align-items: center;
    min-width: 0;
  }

  .bar button,
  .mode {
    border: 0;
    background: none;
  }

  .bar button:disabled {
    opacity: 0.4;
  }

  .count {
    color: var(--fg-muted);
    white-space: nowrap;
  }

  .path {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mode {
    margin-left: auto;
    color: var(--fg-muted);
  }

  /* What Go to definition is doing, or that it found nothing. */
  .notice {
    overflow: hidden;
    color: var(--fg-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mode[aria-pressed="true"] {
    color: var(--fg);
    font-weight: 600;
  }

  /* The diff fills what the tab leaves, and scrolls by itself. */
  .diff {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
