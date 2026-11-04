// The status bar's words for when the open Repository last synced.
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
