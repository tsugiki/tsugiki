<script lang="ts">
  import { openLink } from "$lib/state/browser";
  import type {
    Cloning,
    LastSync,
    MirrorProblem,
    NewVersion,
    Status,
    SyncActivity,
  } from "$lib/state/status.svelte";
  import { type ChordStart, currentOs, keyLabel } from "./keys";
  import {
    cloningWords,
    elsewhereWords,
    mirrorWords,
    pausedWords,
    questionWords,
    syncedWords,
    syncingWords,
  } from "./synced";

  const MINUTE = 60_000;
  const GIT_DOWNLOADS = "https://git-scm.com/downloads";

  let {
    status,
    error,
    visible = true,
    problems = [],
    lastSync = null,
    syncActivity = null,
    cloning = null,
    mirrorProblem = null,
    mirrorQuestion = null,
    newVersion = null,
    chord = null,
    onrefresh = () => {},
    oncheckagain = () => {},
    onquestion = () => {},
  }: {
    status: Status | undefined;
    error?: string | undefined;
    visible?: boolean;
    problems?: string[];
    lastSync?: LastSync | null;
    syncActivity?: SyncActivity | null;
    cloning?: Cloning | null;
    mirrorProblem?: MirrorProblem | null;
    mirrorQuestion?: string | null;
    /** A version newer than the running one, whose release page a click opens. */
    newVersion?: NewVersion | null;
    /** The first key of the chord whose second key Tsugiki waits for. */
    chord?: ChordStart | null;
    onrefresh?: () => void;
    oncheckagain?: () => void;
    /** Asks where `repository`'s mirror gets its objects. */
    onquestion?: (repository: string) => void;
  } = $props();

  /** The problem that kept the browser from opening git's downloads, until the next try. */
  let browserProblem = $state<string>();
  const getGit = async () => {
    browserProblem = await openLink(GIT_DOWNLOADS);
  };
  /** The problem that kept the browser from opening the new version's release page, until the next try. */
  let releaseProblem = $state<string>();
  const openRelease = async (release: NewVersion) => {
    releaseProblem = await openLink(release.page);
  };

  // "Synced 2 min ago" has to move on by itself between syncs.
  let now = $state(new Date());
  $effect(() => {
    const ticking = setInterval(() => {
      now = new Date();
    }, MINUTE);
    return () => {
      clearInterval(ticking);
    };
  });

  const offline = $derived(status?.state === "offline" || status?.state === "offline_as");
  // Hiding the bar can't hide these: nothing else would tell the user.
  const showStatus = $derived(visible || offline || status?.state === "problem" || Boolean(error));
  const paused = $derived(pausedWords(syncActivity));
  // Hiding the bar can't hide why the Files tab has no diff either.
  const mirror = $derived(mirrorWords(mirrorProblem));
  const needsGit = $derived(mirrorProblem?.kind === "no_git" || mirrorProblem?.kind === "old_git");
  const showRelease = $derived(newVersion !== null && visible);
  // The browser's problem goes with the button that met it.
  const listedProblems = $derived(
    [
      ...problems,
      paused,
      mirror,
      needsGit ? browserProblem : undefined,
      showRelease ? releaseProblem : undefined,
    ]
      .filter((words) => Boolean(words))
      .join(" · "),
  );
  /** Whether anything shows before the question's line, which then needs a " · " before it. */
  const beforeQuestion = $derived(showStatus || Boolean(listedProblems));
  // A sync of the Repository open before the window switched says nothing about the one open now.
  const syncing = $derived(
    syncActivity?.open?.repository === lastSync?.repository ? syncingWords(syncActivity) : null,
  );
  const elsewhere = $derived(elsewhereWords(syncActivity));
  const clone = $derived(cloningWords(cloning));
  const syncWords = $derived(
    [syncing ?? syncedWords(lastSync?.at ?? null, now), clone, elsewhere]
      .filter((words) => words !== null)
      .join(" · "),
  );
</script>

<!-- One row, so the sync words take the room they need and the status gets the rest. -->
<div class="bar">
  <!-- Always in the page, because screen readers only announce changes in a live region that already
       exists. -->
  <footer role="status" class:offline class:empty={!showStatus && !listedProblems && mirrorQuestion === null}>
    {#if showStatus}
      {#if error}
        Tsugiki can't read its status: {error}
      {:else if status?.state === "problem"}
        {status.problem.message}
      {:else if status?.state === "offline"}
        Offline: read-only
      {:else if status?.state === "offline_as"}
        {`${status.last_login} · Offline: read-only`}
      {:else if status?.state === "online"}
        {status.login}
      {:else if status?.state === "connecting_as"}
        {`${status.last_login} · Connecting…`}
      {:else}
        Connecting…
      {/if}
    {/if}
    <span>{showStatus && listedProblems ? `· ${listedProblems}` : listedProblems}</span>
    <!-- Hiding the bar can't hide the question the Files tab waits for. -->
    {#if mirrorQuestion !== null}
      {@const repository = mirrorQuestion}
      {#if beforeQuestion}·{/if}
      <button
        class="question"
        onclick={() => {
          onquestion(repository);
        }}>{questionWords(repository)}</button
      >
    {/if}
  </footer>
  {#if needsGit}
    <button onclick={() => void getGit()}>Get git</button>
  {/if}
  {#if mirrorProblem !== null}
    <button onclick={oncheckagain}>Check again</button>
  {/if}
  <!-- Tsugiki doesn't update itself (ADR 0012), so this only opens the release page. -->
  {#if showRelease && newVersion !== null}
    {@const release = newVersion}
    <button onclick={() => void openRelease(release)}>{`Tsugiki ${release.version} is out`}</button>
  {/if}
  <!-- Outside the live region, or a screen reader would read this out each minute, and each page of a
       sync. -->
  {#if lastSync !== null && showStatus}
    <button class:offline onclick={onrefresh}>{syncWords}</button>
  {/if}
  {#if chord !== null && visible}
    <kbd class:offline>{keyLabel(chord, currentOs())}</kbd>
  {/if}
</div>

<style>
  .bar {
    position: fixed;
    inset: auto 0 0;
    display: flex;
  }

  footer {
    flex: 1;
    min-width: 0;
    padding: 2px 8px;
    font-size: 12px;
    background: var(--bg-chrome);
    color: var(--fg-chrome);
    border-top: 1px solid var(--border);
  }

  button,
  kbd {
    flex: none;
    align-self: flex-end;
    white-space: nowrap;
    padding: 2px 8px;
    border: none;
    font-size: 12px;
    background: var(--bg-chrome);
    color: var(--fg-chrome);
    border-top: 1px solid var(--border);
  }

  /* A part of the bar, so square, unlike the window's other buttons. */
  button {
    border-radius: 0;
  }

  /* A line of the status, which reads as a link to the question. */
  .question {
    padding: 0;
    border: none;
    white-space: normal;
    font: inherit;
    color: inherit;
    background: none;
    text-decoration: underline;
  }

  .offline {
    background: var(--warn-bg);
    color: var(--warn-fg);
  }

  /* An empty bar takes no room and draws no line. */
  .empty {
    padding: 0;
    border: none;
  }
</style>
