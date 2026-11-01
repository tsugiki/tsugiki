// PROTOTYPE — throwaway. What the status bar reports. Pending reviews come from the real user state;
// sync, connection, token, rate limit and background jobs have no backend here, so a scenario
// (?status=, or the floating bar) stands in for them. "Refresh now" plays a short fake sync.
import { REPOS, userOf, app, say } from "./app.svelte";

export const SCENARIOS: [string, string][] = [
  ["normal", "All synced"], ["syncing", "Syncing"], ["jobs", "Background jobs"],
  ["offline", "Offline"], ["token", "Token expired"], ["rate", "Rate limit low"],
];

export interface Job { label: string; done: number; total: number; unit: string }
export const status = $state({
  scenario: "normal",
  lastSync: Date.now(),
  now: Date.now(),
  /** A refresh the user asked for: 0..1 while it runs, null otherwise. */
  refreshing: null as number | null,
});
setInterval(() => (status.now = Date.now()), 15_000);

export function refreshNow() {
  if (status.scenario === "offline") return say("Still offline: gh-rev shows the mirror and the cached data until GitHub answers again");
  if (status.refreshing != null) return;
  status.refreshing = 0;
  const t = setInterval(() => {
    status.refreshing = Math.min(1, (status.refreshing ?? 0) + 0.2);
    if (status.refreshing >= 1) { clearInterval(t); status.refreshing = null; status.lastSync = status.now = Date.now(); }
  }, 300);
}

/** A problem the user must see even with the status bar switched off. */
export const critical = () => status.scenario === "offline" || status.scenario === "token";

/** The scenario's background jobs, as the Rust side would report them. */
export function jobs(): Job[] {
  if (status.refreshing != null) return [{ label: `Refreshing ${app.repo ?? "Repositories"}`, done: Math.round(status.refreshing * 40), total: 40, unit: "items" }];
  if (status.scenario === "syncing") return [{ label: `Refreshing ${app.repo ?? REPOS[0]}`, done: 18, total: 40, unit: "items" }];
  if (status.scenario === "jobs") return [
    { label: "Fetching the python/cpython mirror", done: 64, total: 100, unit: "%" },
    { label: "First sync of sergey-miryanov/newproject", done: 120, total: 400, unit: "Issues and PRs" },
    { label: "Checking 17 Stale reviews (patch-id)", done: 5, total: 17, unit: "PRs" },
  ];
  return [];
}

/** Unsent Pending reviews in every Repository, newest-numbered first. */
export function pendingReviews(): { repo: string; pr: number; comments: number }[] {
  return REPOS.flatMap((repo) => [...userOf(repo).pending].filter(([, c]) => c.length).map(([pr, c]) => ({ repo, pr, comments: c.length })))
    .sort((a, b) => b.pr - a.pr);
}
