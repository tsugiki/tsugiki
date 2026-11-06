// The status bar's words for when the open Repository last synced, for a sync under way, and for the other
// Repositories syncing.
import type { SyncActivity } from "$lib/state/status.svelte";

type OpenSync = NonNullable<SyncActivity["open"]>;

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

/** The wait for GitHub's points, while there is one. */
export function pausedWords(activity: SyncActivity | null): string | null {
  if (activity?.paused_until === null || activity === null) return null;
  const until = new Date(activity.paused_until);
  const time = until.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return `Sync paused until ${time}: GitHub's rate limit is low`;
}
