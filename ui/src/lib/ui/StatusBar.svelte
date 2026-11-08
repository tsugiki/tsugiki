<script lang="ts">
  import { openLink } from "$lib/state/browser";
  import { chooseMirrorSource, pickClone, type MirrorSource } from "$lib/state/mirror-source";
  import type { Cloning, LastSync, MirrorProblem, Status, SyncActivity } from "$lib/state/status.svelte";
  import { pickedWords } from "./clone-words";
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
    onrefresh = () => {},
    oncheckagain = () => {},
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
    onrefresh?: () => void;
    oncheckagain?: () => void;
  } = $props();

  /** The problem that kept the browser from opening git's downloads, until the next try. */
  let browserProblem = $state<string>();
  const getGit = async () => {
    browserProblem = await openLink(GIT_DOWNLOADS);
  };

  /** The reason the user's answer didn't take, kept with the Repository it answered for: a clone that can't
   * lend, or an answer `machine.toml` can't keep. */
  let cloneProblem = $state<{ repository: string; words: string }>();
  /** Answers the question for `repository`. The core starts its clone, and the next question comes as an
   * event. */
  const choose = async (repository: string, source: MirrorSource) => {
    const problem = await chooseMirrorSource(repository, source);
    cloneProblem = problem === undefined ? undefined : { repository, words: problem };
  };
  const borrow = async (repository: string) => {
    const picked = await pickClone(repository);
    if (picked.kind === "lends") return choose(repository, { kind: "borrow", folder: picked.folder });
    const words = pickedWords(picked, repository);
    cloneProblem = words === null ? undefined : { repository, words };
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
  const question = $derived(questionWords(mirrorQuestion));
  // A refusal goes with the question it answered.
  const refused = $derived(cloneProblem?.repository === mirrorQuestion ? cloneProblem?.words : undefined);
  const needsGit = $derived(mirrorProblem?.kind === "no_git" || mirrorProblem?.kind === "old_git");
  // The browser's problem goes with the button that met it.
  const listedProblems = $derived(
    [...problems, paused, mirror, needsGit ? browserProblem : undefined, question, refused]
      .filter((words) => Boolean(words))
      .join(" · "),
  );
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
  <footer role="status" class:offline class:empty={!showStatus && !listedProblems}>
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
  </footer>
  {#if needsGit}
    <button onclick={() => void getGit()}>Get git</button>
  {/if}
  {#if mirrorProblem !== null}
    <button onclick={oncheckagain}>Check again</button>
  {/if}
  {#if mirrorQuestion !== null}
    {@const repository = mirrorQuestion}
    <button onclick={() => void borrow(repository)}>Borrow from a clone…</button>
    <button
      onclick={() => {
        void choose(repository, { kind: "download" });
      }}>Download</button
    >
  {/if}
  <!-- Outside the live region, or a screen reader would read this out each minute, and each page of a
       sync. -->
  {#if lastSync !== null && showStatus}
    <button class:offline onclick={onrefresh}>{syncWords}</button>
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
    background: #e8e8e8;
    color: #222;
  }

  button {
    flex: none;
    align-self: flex-end;
    white-space: nowrap;
    padding: 2px 8px;
    border: none;
    font-size: 12px;
    background: #e8e8e8;
    color: #222;
    cursor: pointer;
  }

  .offline {
    background: #a85000;
    color: #fff;
  }

  .empty {
    padding: 0;
  }
</style>
