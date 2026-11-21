<script lang="ts">
  import { LinkOpener } from "$lib/state/link-opener.svelte";
  import type {
    Cloning,
    HourPoints,
    LastSync,
    MirrorProblem,
    NewVersion,
    PointsBack,
    RepositoryMirror,
    Status,
    SyncActivity,
  } from "$lib/state/status.svelte";
  import { currentOs, keyLabel } from "./keys";
  import { pointsTip, pointsWords } from "./points";
  import {
    cloningWords,
    elsewhereWords,
    mirrorWords,
    offlineCloneWords,
    pausedWords,
    pointsBackWords,
    questionWords,
    syncedWords,
    syncingWords,
  } from "./synced";
  import { minuteClock } from "./clock.svelte";

  const GIT_DOWNLOADS = "https://git-scm.com/downloads";

  let {
    status,
    error,
    visible = true,
    problems = [],
    lastSync = null,
    syncActivity = null,
    hourPoints = null,
    pointsBack = null,
    cloning = null,
    mirrorStates = [],
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
    /** The hour's GitHub points, which the bar shows once GitHub's first answer has said. */
    hourPoints?: HourPoints | null;
    /** The time GitHub's points come back, while the rate limit holds every request to GitHub. */
    pointsBack?: PointsBack | null;
    cloning?: Cloning | null;
    /** The state of each Repository's mirror, which says when a clone waits for the network. */
    mirrorStates?: RepositoryMirror[];
    mirrorProblem?: MirrorProblem | null;
    mirrorQuestion?: string | null;
    /** A version newer than the running one, whose release page a click opens. */
    newVersion?: NewVersion | null;
    /** The first key of the chord whose second key Tsugiki waits for. */
    chord?: string | null;
    onrefresh?: () => void;
    oncheckagain?: () => void;
    /** Asks where `repository`'s mirror gets its objects. */
    onquestion?: (repository: string) => void;
  } = $props();

  /** Opens git's downloads, and keeps what kept the browser from it until the next try. */
  const gitPage = new LinkOpener();
  /** Opens the new version's release page, and keeps what kept the browser from it until the next try. */
  const releasePage = new LinkOpener();

  // "Synced 2 min ago" has to move on by itself between syncs.
  const now = minuteClock();

  // The core ends the hour at its reset by GitHub's clock, so the part goes then.
  const points = $derived(visible ? hourPoints : null);
  const offline = $derived(status?.state === "offline" || status?.state === "offline_as");
  // Asking GitHub again after being offline, which the bar showed, so it goes on showing while it asks.
  const reconnecting = $derived(status?.state === "reconnecting" || status?.state === "reconnecting_as");
  // Hiding the bar can't hide these: nothing else would tell the user.
  const showStatus = $derived(
    visible || offline || reconnecting || status?.state === "problem" || Boolean(error),
  );
  // With no sync to pause, the bar still says why nothing asks GitHub, as a read or a fetch points here.
  const paused = $derived(
    pausedWords(syncActivity) ?? (pointsBack === null ? null : pointsBackWords(pointsBack.at)),
  );
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
      needsGit ? gitPage.problem : undefined,
      showRelease ? releasePage.problem : undefined,
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
  // The core says a clone waits for the network only while none is under way.
  const clone = $derived(cloningWords(cloning) ?? offlineCloneWords(mirrorStates));
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
      {:else if status?.state === "connecting_as" || status?.state === "reconnecting_as"}
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
    <button onclick={() => void gitPage.openLink(GIT_DOWNLOADS)}>Get git</button>
  {/if}
  {#if mirrorProblem !== null}
    <button onclick={oncheckagain}>Check again</button>
  {/if}
  <!-- Tsugiki doesn't update itself (ADR 0012), so this only opens the release page. -->
  {#if showRelease && newVersion !== null}
    {@const release = newVersion}
    <button onclick={() => void releasePage.openLink(release.page)}
      >{`Tsugiki ${release.version} is out`}</button
    >
  {/if}
  <!-- Outside the live region, or a screen reader would read this out each minute, and each page of a
       sync. -->
  {#if lastSync !== null && showStatus}
    <button class:offline onclick={onrefresh}>{syncWords}</button>
  {/if}
  <!-- Outside the live region too, as it changes with each answer from GitHub. A screen reader can't reach a
       tooltip, so it reads the words and the tooltip from a copy of its own, once. -->
  {#if points !== null}
    {@const words = pointsWords(points)}
    {@const tip = pointsTip(points)}
    <span class="points" class:offline
      ><span aria-hidden="true" title={tip}>{words}</span><span class="for-screen-readers"
        >{`${words} · ${tip}`}</span
      ></span
    >
  {/if}
  {#if chord !== null && visible}
    <kbd class:offline>{keyLabel(chord, currentOs())}</kbd>
  {/if}
</div>

<style>
  /* The last row of the window, under the screen. */
  .bar {
    display: flex;
    flex: none;
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
  .points,
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
