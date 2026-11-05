// The status bar's words for when the open Repository last synced, and for a sync under way.
import type { SyncActivity } from "$lib/state/status.svelte";

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
function loadWords(load: NonNullable<SyncActivity["load"]>): string {
  const issues = load.issues_total === null ? "" : ` of ${String(load.issues_total)}`;
  return `Issues ${String(load.issues_read)}${issues} · PRs ${String(load.prs_fetched)} of ${String(load.prs_known)} so far`;
}

/** A first sync's part and how far it has got. A refresh has no words: it says only when it has to wait. */
export function syncingWords(activity: SyncActivity | null): string | null {
  if (activity === null || activity.part === "refresh") return null;
  if (activity.load) return `${PART_WORDS[activity.part]} · ${loadWords(activity.load)}`;
  if (activity.pages === 0) return PART_WORDS[activity.part];
  const pages = activity.pages === 1 ? "1 page" : `${String(activity.pages)} pages`;
  return `${PART_WORDS[activity.part]} · ${pages}`;
}

/** The wait for GitHub's points, while there is one. */
export function pausedWords(activity: SyncActivity | null): string | null {
  if (activity?.paused_until === null || activity === null) return null;
  const until = new Date(activity.paused_until);
  const time = until.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Sync paused until ${time}: GitHub's rate limit is low`;
}
