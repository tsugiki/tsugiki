// The status bar's words for when the open Repository last synced, for a sync under way, for the other
// Repositories syncing, and for a mirror's clone and what keeps one from starting.
import type { Cloning, MirrorProblem, RepositoryMirror, SyncActivity } from "$lib/state/status.svelte";

type OpenSync = NonNullable<SyncActivity["open"]>;

/** How far a clone has got once GitHub has started sending. */
type CloneUnderWay = Exclude<Cloning["progress"], { stage: "starting" }>;

const MINUTE_MS = 60_000;
const MINUTES_IN_HOUR = 60;
const MINUTES_IN_DAY = 1440;

export function syncedWords(at: string | null, now: Date): string {
  if (at === null) return "Not synced yet";
  const minutes = Math.floor((now.getTime() - new Date(at).getTime()) / MINUTE_MS);
  if (minutes < 1) return "Synced just now";
  if (minutes < MINUTES_IN_HOUR) return `Synced ${String(minutes)} min ago`;
  if (minutes < MINUTES_IN_DAY) return `Synced ${String(Math.floor(minutes / MINUTES_IN_HOUR))} h ago`;
  return `Synced ${String(Math.floor(minutes / MINUTES_IN_DAY))} d ago`;
}

const PART_WORDS = {
  your_work: "Syncing your work",
  window: "Syncing recent items",
  older_open: "Syncing older open items",
  trees: "Syncing Issue trees",
} as const;

/**
 * A first sync's progress through its queue. It learns of more PRs as it reads, so their count says "so far".
 */
function loadWords(load: NonNullable<OpenSync["load"]>): string {
  const issues = load.issues_total === null ? "" : ` of ${String(load.issues_total)}`;
  return `Issues ${String(load.issues_read)}${issues} · PRs ${String(load.prs_fetched)} of ${String(load.prs_known)} so far`;
}

/** A first sync's part and how far it has got. A refresh has no words: it says only when it has to wait. */
export function syncingWords(activity: SyncActivity | null): string | null {
  const open = activity?.open;
  if (!open || open.part === "refresh") return null;
  if (open.load) return `${PART_WORDS[open.part]} · ${loadWords(open.load)}`;
  if (open.pages === 0) return PART_WORDS[open.part];
  const pages = open.pages === 1 ? "1 page" : `${String(open.pages)} pages`;
  return `${PART_WORDS[open.part]} · ${pages}`;
}

/** The other Repositories syncing, which take turns with the open one: the next by name, then a count. */
export function elsewhereWords(activity: SyncActivity | null): string | null {
  const [next, ...rest] = activity?.elsewhere ?? [];
  if (next === undefined) return null;
  return rest.length === 0 ? `${next} syncing` : `${next} and ${String(rest.length)} more syncing`;
}

/** The stage a clone is at, after it has started. Indexing what came down takes about as long as receiving
 * it. */
export function cloneStage(progress: CloneUnderWay): "Cloning" | "Indexing" {
  return progress.stage === "receiving" ? "Cloning" : "Indexing";
}

/** The oldest git a mirror works with (ADR 0003). */
export const OLDEST_GIT = "git 2.30";

/** A mirror's clone and how far it has got. */
export function cloningWords(cloning: Cloning | null): string | null {
  if (cloning === null) return null;
  const { repository, progress } = cloning;
  if (progress.stage === "starting") return `Cloning ${repository}`;
  return `${cloneStage(progress)} ${repository} ${String(progress.percent)}%`;
}

/** The words for the clones that wait for the network, as GitHub can't be reached (#390), or nothing if
 * none waits. */
export function offlineCloneWords(mirrors: RepositoryMirror[]): string | null {
  const offline = mirrors.filter((mirror) => mirror.state.state === "offline");
  const [first] = offline;
  if (first === undefined) return null;
  if (offline.length === 1) return `Cloning ${first.repository} starts once GitHub answers`;
  return `${String(offline.length)} clones start once GitHub answers`;
}

const MB_IN_GB = 1024;

/** A size in megabytes, in gigabytes from one up. */
export function sizeWords(mb: number): string {
  return mb < MB_IN_GB ? `${String(mb)} MB` : `${(mb / MB_IN_GB).toFixed(1)} GB`;
}

/** The words for what keeps a mirror from being made. The Files tab needs one. */
export function mirrorWords(problem: MirrorProblem | null): string | null {
  if (problem === null) return null;
  if (problem.kind === "no_git")
    return `The Files tab needs ${OLDEST_GIT} or newer, and Tsugiki can't find git`;
  if (problem.kind === "old_git") return `The Files tab needs ${OLDEST_GIT} or newer: ${problem.version}`;
  const { repository, folder, needed_mb, free_mb } = problem;
  return `No room to clone ${repository}: it needs ${sizeWords(needed_mb)} on the disk with ${folder}, which has ${sizeWords(free_mb)} free`;
}

/** The status bar's line for the first Repository whose mirror waits to hear where it gets its objects. A
 * click on it asks the question. */
export function questionWords(repository: string): string {
  return `${repository}'s mirror waits for your answer`;
}

/** The wait for GitHub's points, while there is one. */
export function pausedWords(activity: SyncActivity | null): string | null {
  if (activity?.paused_until === null || activity === null) return null;
  const until = new Date(activity.paused_until);
  const time = until.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Sync paused until ${time}: GitHub's rate limit is low`;
}
