// The Workbench's words for what the core says about an Issue or a PR.
import {
  treeRows,
  type Attention,
  type BackportKind,
  type CantCheck,
  type CiState,
  type IssueItem,
  type ListedItem,
  type MuteAuthor,
  type PrState,
  type Seen,
  type TreeEntry,
  type WorkbenchRow,
  type WorkReason,
  type Why,
  type YourReview,
} from "$lib/state/workbench.svelte";
import { counted } from "$lib/state/counted";
import { showsAsDraft } from "./octicons";
import { elapsed, stampOf } from "./time";

const REASONS: Record<WorkReason, string> = {
  review_request: "Review request",
  own_pr: "Own PR",
  reviewed: "Reviewed",
};

const STATES: Record<PrState, string> = {
  OPEN: "Open",
  CLOSED: "Closed",
  MERGED: "Merged",
};

const BACKPORTS: Record<BackportKind, string> = {
  bot: "Bot backport",
  manual: "Manual backport",
};

const CI: Record<CiState, string> = {
  EXPECTED: "CI expected",
  ERROR: "CI error",
  FAILURE: "CI failed",
  PENDING: "CI running",
  SUCCESS: "CI passed",
};

/** The words an Alert would use for one reason an item Needs attention. */
function saying(why: Why): string {
  switch (why.kind) {
    case "review_requested":
      return "Review requested from you";
    case "assigned":
      return "Assigned to you";
    case "stale_review":
      return "Changed since your Review";
    case "ci_failed":
      return why.checks.length > 0 ? `CI failed: ${why.checks.join(", ")}` : "CI failed";
    case "ci_passed":
      return "CI passed";
    case "changes_requested":
      return `Changes requested by ${why.by}`;
    case "approved":
      return `Approved by ${why.by}`;
    case "reviewed":
      return `Reviewed by ${why.by}`;
    case "comment":
      return `New comment from ${why.by}`;
    case "commits":
      return `New commits from ${why.by}`;
    case "ready_for_review":
      return `Ready for review, from ${why.by}`;
    case "mentioned":
      return "You were mentioned";
    case "backport_failed":
      return `Bot backport to ${why.target} failed`;
    case "marked_unseen":
      return "Marked unseen";
  }
}

/** Whether an item's header offers Mark Seen: it has New activity, or the user marked it unseen. Otherwise
 * the header offers Mark Unseen. */
export const offersMarkSeen = (seen: Seen) => seen.state !== "seen";

/** The reasons an item Needs attention, the newest first. */
export const preview = (attention: Attention) => attention.why.map(saying).join(" · ");

/** The words of the Issue view's and the PR view's Mute author button: the author it mutes or unmutes, or
 * "Mute author" while it's off (#494). */
export const muteAuthorWords = (offered: MuteAuthor) =>
  offered.kind === "off" ? "Mute author" : `${offered.kind === "mute" ? "Mute" : "Unmute"} ${offered.login}`;

const AGO_UNITS = { minute: "m", hour: "h", day: "d" } as const;

/** The time since `updatedAt`: "just now", "3m ago", "5h ago" or "2d ago". */
export function ago(updatedAt: string, now: Date): string {
  const time = elapsed(updatedAt, now);
  return time.unit === "now" ? "just now" : `${String(time.count)}${AGO_UNITS[time.unit]} ago`;
}

/** A PR's or a Backport's update, for the tooltip beside its "… ago". */
export const updatedTip = (row: { updated_at: string }) => `Updated ${stampOf(row.updated_at)}`;

/** An item's time, for the tooltip beside its "… ago" (#1066), saying what it measures. A Standalone PR's
 * or Backport's is its own update. An Issue's tooltip lists its own update and each PR's in its tree, each
 * with its date and time, so the user sees which update put it in the list, and when. */
export function ageTip(item: ListedItem): string {
  if (item.kind === "placeholder") return "";
  if (item.kind !== "issue") {
    const what = item.kind === "standalone_backport" ? "Backport" : "PR";
    return `${what} updated ${stampOf(item.updated_at)}`;
  }
  const lines = [
    `Issue updated ${stampOf(item.issue_updated_at)}`,
    ...treeRows(item.pull_requests).map((row) => `#${String(row.number)} updated ${stampOf(row.updated_at)}`),
  ];
  return lines.join("\n");
}

/** How many comments an Issue, a PR or a review thread has. */
export const commentCount = (count: number) => counted(count, "comment", "comments");

/** An Issue's or a PR's first line. */
export const headline = (item: { number: number; title: string }) => `#${String(item.number)} ${item.title}`;

const prCount = (count: number) => counted(count, "PR", "PRs");

/** How many PRs an Issue's tree holds, Backports included. */
export const treeCount = (issue: IssueItem) => prCount(treeRows(issue.pull_requests).length);

/** An item's third line while nothing in it waits on the user: how many of an Issue's PRs are open, or a
 * PR's facts. */
export function summary(item: ListedItem): string {
  if (item.kind === "placeholder") return "";
  if (item.kind !== "issue") return details(item);
  return item.pull_requests.length > 0 ? openOfAll(item.pull_requests) : "No PR yet";
}

/** How many of an Issue's PRs are open, of all of them, Backports included. */
function openOfAll(entries: TreeEntry[]): string {
  const rows = treeRows(entries);
  const open = rows.filter((row) => row.state === "OPEN").length;
  return `${String(open)} open of ${prCount(rows.length)}`;
}

/** How a word of a PR's state stands out: a Review request, CI that failed or passed, a Stale review, or not
 * at all. */
export type WordTone = "request" | "bad" | "good" | "plain";

/** A word of a PR's state, and what its hover says, if it says more. */
export type StateWord = { words: string; tone: WordTone; hover?: string };

const fileCount = (count: number) => counted(count, "file", "files");

/** Why the Stale review check can't run, for the hover of a Stale review it couldn't check, `cant` naming
 * the Review. */
function cantCheckWords(reason: CantCheck, cant = "Tsugiki can't check your Review"): string {
  switch (reason.kind) {
    case "cloning":
      return `${cant} until the mirror is cloned`;
    case "broken":
      return `${cant} until the mirror, which git said was broken, is cloned again`;
    case "no_git":
      return `${cant} without git`;
    case "old_git":
      return `${cant} with ${reason.version}: it needs git ${reason.needed} or newer`;
    case "refused":
      return `${cant}: GitHub refused to send the commit it covered`;
  }
}

/** The PR view's line over both tabs while the user's Review is a Stale review (#387): the commit the Review
 * covered, then how many files changed since and how many of those only in whitespace, or why the check
 * can't run. A Bot backport's line names its Origin PR, `origin`, whose Review it shares. Nothing while the
 * PR is Unreviewed or Reviewed. */
export function staleReviewLine(review: YourReview, origin: number | null): string | undefined {
  if (review.kind !== "stale_review" && review.kind !== "cant_check") return undefined;
  const of = origin === null ? "" : ` of #${String(origin)}`;
  const yours = `Your Review${of} at ${review.reviewed.slice(0, 7)}`;
  if (review.kind === "cant_check") {
    return `${yours} may be stale: ${cantCheckWords(review.reason, "Tsugiki can't check it")}`;
  }
  const whitespace = review.files.filter((file) => file.whitespace_only).length;
  const only = whitespace === 0 ? "" : `, ${String(whitespace)} only in whitespace`;
  return `${yours} is stale: ${fileCount(review.files.length)} changed${only}`;
}

/** The word for where the user's Review stands. A Stale review's hover lists the files changed since the
 * Review, and names the ones whose change is only whitespace, or says why the check can't run. An
 * Unreviewed PR has none. */
function reviewWord(review: YourReview): StateWord[] {
  switch (review.kind) {
    case "unreviewed":
      return [];
    case "reviewed":
      return [{ words: "Reviewed", tone: "plain" }];
    case "stale_review": {
      const files = review.files.map((file) =>
        file.whitespace_only ? `${file.path} (whitespace only)` : file.path,
      );
      const hover = [`Changed since your Review: ${fileCount(files.length)}`, ...files].join("\n");
      return [{ words: "Stale review", tone: "bad", hover }];
    }
    case "cant_check":
      return [{ words: "Stale review", tone: "bad", hover: cantCheckWords(review.reason) }];
  }
}

/** The words of a PR's second line, with only the facts this PR has. A Stale review takes the place of the
 * Reviewed reason. On a Bot backport's row its review state always does: beneath its Origin PR it shares that
 * PR's, whatever the user's Review of the Backport says. */
export function detailWords(row: WorkbenchRow & { backport?: BackportKind }): StateWord[] {
  const stale = row.your_review.kind === "stale_review" || row.your_review.kind === "cant_check";
  const shown = stale || row.backport === "bot";
  const plain = (words: string) => ({ words, tone: "plain" as const });
  return [
    ...row.reasons
      .filter((reason) => !(shown && reason === "reviewed"))
      .map((reason) => plain(REASONS[reason])),
    ...(shown ? reviewWord(row.your_review) : []),
    ...(row.backport ? [plain(BACKPORTS[row.backport])] : []),
    plain(row.author ?? "deleted user"),
    plain(`into ${row.target_branch}`),
    plain(STATES[row.state]),
    ...(showsAsDraft(row) ? [plain("Draft")] : []),
    ...(row.ci ? [plain(CI[row.ci])] : []),
  ];
}

/** A PR's second line, as text. */
export const details = (row: WorkbenchRow & { backport?: BackportKind }) =>
  detailWords(row)
    .map((word) => word.words)
    .join(" · ");

const CI_TONES: Record<CiState, WordTone> = {
  EXPECTED: "plain",
  ERROR: "bad",
  FAILURE: "bad",
  PENDING: "plain",
  SUCCESS: "good",
};

/** A PR's state words in the Issue view and the PR view: its state, CI and where the user stands on it. */
export function prStateWords(row: WorkbenchRow): StateWord[] {
  return [
    { words: STATES[row.state], tone: "plain" as const },
    ...(showsAsDraft(row) ? [{ words: "Draft", tone: "plain" as const }] : []),
    ...(row.ci ? [{ words: CI[row.ci], tone: CI_TONES[row.ci] }] : []),
    ...(row.reasons.includes("review_request")
      ? [{ words: REASONS.review_request, tone: "request" as const }]
      : []),
    ...reviewWord(row.your_review),
  ];
}

/** A Backport in the line its Origin PR folds them into: its branch and state. */
export const backportWords = (row: WorkbenchRow) => `${row.target_branch} ${STATES[row.state]}`;

/** The tooltip of the line that folds merged PRs, which says the latest of their updates (#1066). */
export function foldedTip(entries: TreeEntry[]): string {
  const rows = treeRows(entries);
  const latest = rows.reduce((one, other) =>
    Date.parse(other.updated_at) > Date.parse(one.updated_at) ? other : one,
  );
  return `Latest update of these PRs: ${stampOf(latest.updated_at)}`;
}
