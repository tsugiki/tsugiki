<script lang="ts">
  import { LinkOpener } from "$lib/state/link-opener.svelte";
  import type {
    Account,
    Cloning,
    HourPoints,
    InFlight,
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
  import { minuteClock, secondClock } from "./clock.svelte";
  import { inFlightWords, SLOW_MS } from "./in-flight";
  import { dateTime } from "./time";

  const GIT_DOWNLOADS = "https://git-scm.com/downloads";

  /** What the status bar says of `account`: who is signed in, or why nobody is. Nothing, when GitHub answered
   * without a login and none is known, or when Tsugiki asks GitHub nothing more, as the problem beside it
   * then stands alone (#1070). */
  function accountWords(account: Account | undefined): string {
    switch (account?.state) {
      case "no_token":
      case "refused":
        return account.message;
      case "offline":
        return "Offline";
      case "offline_as":
        return `${account.last_login} · Offline`;
      case "online":
        return account.login;
      case "unconfirmed_as":
        return account.last_login;
      case "unconfirmed":
      case "stopped":
        return "";
      case "connecting_as":
      case "reconnecting_as":
        return `${account.last_login} · Connecting…`;
      case "connecting":
      case "reconnecting":
      case undefined:
        return "Connecting…";
    }
  }

  let {
    status,
    error,
    visible = true,
    problems = [],
    lastSync = null,
    syncActivity = null,
    hourPoints = null,
    pointsBack = null,
    inFlight = null,
    cloning = null,
    mirrorStates = [],
    mirrorProblem = null,
    mirrorQuestion = null,
    newVersion = null,
    readOnly = false,
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
    /** The request to GitHub in flight, which the bar shows once GitHub is slow to answer it. */
    inFlight?: InFlight | null;
    cloning?: Cloning | null;
    /** The state of each Repository's mirror, which says when a clone waits for the network. */
    mirrorStates?: RepositoryMirror[];
    mirrorProblem?: MirrorProblem | null;
    mirrorQuestion?: string | null;
    /** A version newer than the running one, whose release page a click opens. */
    newVersion?: NewVersion | null;
    /** Whether the open Repository is Read-only, which the status says after who is signed in (#398). */
    readOnly?: boolean;
    /** The first key of the chord whose second key Tsugiki waits for. */
    chord?: string | null;
    /** Asks the core for a Refresh, which the sync status and Try again send. */
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
  const clock = minuteClock();
  // The core says only when a request goes and ends, as its thread waits meanwhile, so the bar counts.
  // It ticks only once the part can show: on a bar the settings show, from the request's third second.
  const seconds = secondClock(() => (visible ? (inFlight?.since ?? null) : null), SLOW_MS);
  const waiting = $derived(visible ? inFlightWords(inFlight, seconds.now) : null);

  // The core ends the hour at its reset by GitHub's clock, so the part goes then.
  const points = $derived(visible ? hourPoints : null);
  const account = $derived(status?.account);
  // An unread status may be out of date, so the bar then says only why it can't be read, and no problem.
  const problem = $derived(status?.problem);
  const offline = $derived(account?.state === "offline" || account?.state === "offline_as");
  // Asking GitHub again after being offline, which the bar showed, so it goes on showing while it asks.
  const reconnecting = $derived(account?.state === "reconnecting" || account?.state === "reconnecting_as");
  // Hiding the bar can't hide these: nothing else would tell the user. Without a token, or with one GitHub
  // refused, Tsugiki reads nothing, so the account shows then too.
  const showAccount = $derived(
    visible ||
      offline ||
      reconnecting ||
      account?.state === "no_token" ||
      account?.state === "refused" ||
      Boolean(error),
  );
  // Nor can it hide a problem, which shows alone when the account doesn't.
  const showStatus = $derived(showAccount || Boolean(problem));
  // A read of the token that found none can be tried again at once wherever the account says so, as the
  // sync status, which a click refreshes too, shows only while a Repository is open. A problem beside the
  // account can't take it away (#1062).
  const noToken = $derived(!error && account?.state === "no_token");
  // An unread status says nothing of the open Repository, so Read-only waits for a status. A hidden bar shows
  // a token's problem alone, as Read-only matters only once Tsugiki reads, and so does a session that
  // stopped: the problem says why it asks GitHub nothing more (#1070).
  const readOnlyShown = $derived(
    readOnly && (visible || offline || reconnecting) && account?.state !== "stopped" && !error,
  );
  // Who is signed in, or why nobody is, then Read-only, as it says more of the open Repository, then the
  // problem beside the account.
  const statusWords = $derived(
    [
      showAccount ? accountWords(account) : undefined,
      readOnlyShown ? "Read-only" : undefined,
      problem?.message,
    ]
      .filter((words) => Boolean(words))
      .join(" · "),
  );
  // With no sync to pause, the bar still says why nothing asks GitHub, as a read or a fetch points here.
  const paused = $derived(
    pausedWords(syncActivity) ?? (pointsBack === null ? null : pointsBackWords(pointsBack.at)),
  );
  // Hiding the bar can't hide why the Files tab has no diff either.
  const mirror = $derived(mirrorWords(mirrorProblem));
  const needsGit = $derived(mirrorProblem?.kind === "no_git" || mirrorProblem?.kind === "old_git");
  const showRelease = $derived(newVersion !== null && visible);
  // What follows the status: the problems with a file, the wait and the mirror. The browser's problem goes
  // with the button that met it.
  const listed = $derived(
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
  const beforeQuestion = $derived(showStatus || Boolean(listed));
  // A sync of the Repository open before the window switched says nothing about the one open now.
  const syncing = $derived(
    syncActivity?.open?.repository === lastSync?.repository ? syncingWords(syncActivity) : null,
  );
  const elsewhere = $derived(elsewhereWords(syncActivity));
  // The core says a clone waits for the network only while none is under way.
  const clone = $derived(cloningWords(cloning) ?? offlineCloneWords(mirrorStates));
  const syncWords = $derived(
    [syncing ?? syncedWords(lastSync?.at ?? null, clock.now), clone, elsewhere]
      .filter((words) => words !== null)
      .join(" · "),
  );
  /** The date and the time of the last sync, which the button's tooltip says while the button shows that
   * time (#1066). */
  const syncedAt = $derived(
    lastSync === null || lastSync.at === null || syncing !== null
      ? undefined
      : `Last sync: ${dateTime(lastSync.at)}`,
  );
</script>

<!-- One row, so the sync words take the room they need and the status gets the rest. -->
<div class="bar">
  <!-- Always in the page, because screen readers only announce changes in a live region that already
       exists. -->
  <footer role="status" class:offline class:empty={!showStatus && !listed && mirrorQuestion === null}>
    {#if error}
      Tsugiki can't read its status: {error}
    {:else}
      {statusWords}
    {/if}
    <span>{showStatus && listed ? `· ${listed}` : listed}</span>
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
  <!-- A Refresh reads the token, and the core says "Connecting…" until the read ends, which takes the button
       away. The minute's read leaves the account saying there is no token, and the GitHub thread, which
       waits on gh meanwhile, hears no click until it ends: such a click adds no read, and on a bar the
       settings show, from the read's third second, the part on the right says Tsugiki waits for gh. -->
  {#if noToken}
    <button onclick={onrefresh}>Try again</button>
  {/if}
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
  <!-- Outside the live region, or a screen reader would read it out each second. The tooltip holds what
       a long subject's ellipsis cuts. -->
  {#if waiting !== null}
    <span class="in-flight" class:offline title={`${waiting.asking}${waiting.waited}`}
      ><span class="asking">{waiting.asking}</span><span class="waited">{waiting.waited}</span></span
    >
  {/if}
  <!-- Outside the live region, or a screen reader would read this out each minute, and each page of a
       sync. -->
  {#if lastSync !== null && showStatus}
    <button class:offline title={syncedAt} onclick={onrefresh}>{syncWords}</button>
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
  .in-flight,
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

  /* One width whatever it asks, so the bar doesn't jump as the subject changes, and in a narrow window no
     more of the bar than leaves the status room. */
  .in-flight {
    display: flex;
    width: 30em;
    max-width: 30%;
  }

  /* A long subject ends in an ellipsis, and the seconds stay. */
  .asking {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .waited {
    flex: none;
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
