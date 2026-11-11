// PROTOTYPE, throwaway (#367): writing a comment, building a Pending review and submitting it, three ways,
// switched from the floating bar. Nothing reaches GitHub: every write waits a moment and then does what the
// bar's "Pretend" choice says. All of it lives in memory, so a reload clears it.
//
// A: the composer opens inline under the line, drafts show in place, a bar above the diff counts them and
//    Submit opens a dialog.
// B: the composer docks in a panel below the diff with the lines quoted; the diff only marks the lines.
// C: a rail beside the diff lists the drafts, takes the composer at its top, and keeps the Submit form open
//    at its foot.

import { SvelteMap } from "svelte/reactivity";
import type { PrFilesState } from "$lib/state/pr-files.svelte";

export type Variant = "A" | "B" | "C";
export const VARIANTS: { key: Variant; name: string }[] = [
  { key: "A", name: "Inline in the diff, Submit dialog" },
  { key: "B", name: "Panel below the diff" },
  { key: "C", name: "Rail beside the diff" },
];

export type Simulate =
  | "normal"
  | "slow"
  | "not_sent"
  | "unknown"
  | "moved"
  | "refused"
  | "read_only"
  | "own_pr"
  | "closed"
  | "github_pending"
  | "submitted_elsewhere"
  | "placed_newer";
export const SIMULATIONS: { key: Simulate; name: string }[] = [
  { key: "normal", name: "Writes go through" },
  { key: "slow", name: "Writes take 3 s" },
  { key: "not_sent", name: "Next write: not sent" },
  { key: "unknown", name: "Next write: times out" },
  { key: "moved", name: "Next write: lines changed in a newer push" },
  { key: "refused", name: "Next write: refused, token can't write" },
  { key: "read_only", name: "Read-only Repository" },
  { key: "own_pr", name: "Your own PR" },
  { key: "closed", name: "Closed PR" },
  { key: "github_pending", name: "Pending review begun on github.com" },
  { key: "submitted_elsewhere", name: "Submit: review gone on GitHub" },
  { key: "placed_newer", name: "A draft GitHub placed on a newer push" },
];

/** GitHub's limit on a comment's body, in UTF-8 bytes (#373, live try 14). */
export const BODY_LIMIT = 262_144;
/** The composer starts counting bytes this close to the limit. */
export const COUNT_FROM = BODY_LIMIT - 10_000;

export type Side = "LEFT" | "RIGHT";

/** One line a comment covers, as the diff showed it. */
export type Line = { row: number; side: Side; line: number; text: string; kind: "added" | "removed" | "context" };

/** What a composer writes on. */
export type Target =
  | { kind: "lines"; path: string; lines: Line[] }
  | { kind: "file"; path: string; row: number }
  | { kind: "reply"; thread: string; path: string; to: string; row: number | undefined }
  | { kind: "conversation"; label: string };

export type ComposerState =
  | { kind: "editing" }
  | { kind: "sending" }
  | { kind: "not_sent"; why: string }
  | { kind: "unknown" }
  | { kind: "moved" }
  | { kind: "refused" };

export type Composer = {
  target: Target;
  body: string;
  state: ComposerState;
  /** The draft it edits, when it edits one. */
  editing: number | undefined;
};

export type Draft = {
  id: number;
  target: Extract<Target, { kind: "lines" | "file" | "reply" }>;
  body: string;
  /** Begun on github.com, so its lines came from there. */
  fromGitHub: boolean;
  /** GitHub placed it against a newer push than the diff shows. */
  checkLine: boolean;
};

export type ReviewEvent = "COMMENT" | "APPROVE" | "REQUEST_CHANGES";
export const EVENTS: { key: ReviewEvent; name: string; help: string }[] = [
  { key: "COMMENT", name: "Comment", help: "Feedback without approving or blocking." },
  { key: "APPROVE", name: "Approve", help: "The PR can be merged as it is." },
  { key: "REQUEST_CHANGES", name: "Request changes", help: "Feedback that must be addressed before merging." },
];

export type Submitted = { event: ReviewEvent; commit: string; comments: number; at: Date };

/** The key Unsent text is kept under: one per composer place (#366). */
export const placeKey = (target: Target): string => {
  switch (target.kind) {
    case "lines": {
      const end = target.lines.at(-1);
      return `line:${target.path}:${end?.side ?? ""}:${String(end?.line ?? 0)}`;
    }
    case "file":
      return `file:${target.path}`;
    case "reply":
      return `reply:${target.thread}`;
    case "conversation":
      return `conversation:${target.label}`;
  }
};

/** Where a target is, in words. */
export const targetWords = (target: Target): string => {
  switch (target.kind) {
    case "lines": {
      const [first, last] = [target.lines[0], target.lines.at(-1)];
      if (!first || !last) return target.path;
      const sign = (one: Line) => (one.side === "LEFT" ? "−" : "+");
      if (first === last) return `${target.path} · line ${sign(first)}${String(first.line)}`;
      const mixed = first.side !== last.side;
      const range = mixed
        ? `${sign(first)}${String(first.line)} to ${sign(last)}${String(last.line)}`
        : `${sign(first)}${String(first.line)}–${String(last.line)}`;
      return `${target.path} · lines ${range}`;
    }
    case "file":
      return `${target.path} · the whole file`;
    case "reply":
      return `Reply to ${target.to}${target.path ? ` · ${target.path}` : ""}`;
    case "conversation":
      return target.label;
  }
};

/** Why a `suggestion` can't be offered here, or undefined when it can (#373, live try 13). */
export const noSuggestion = (target: Target): string | undefined => {
  if (target.kind === "file") return "github.com can't apply a suggestion on a whole file.";
  if (target.kind === "conversation") return "A suggestion goes on lines of the diff.";
  if (target.kind === "reply") return undefined;
  const [first, last] = [target.lines[0], target.lines.at(-1)];
  if (first?.kind === "removed" && first === last) return "github.com can't apply a suggestion on a removed line.";
  if (first?.kind === "removed") return "github.com can't apply a suggestion on a range that starts on a removed line.";
  if (last?.kind === "removed") return "github.com can't apply a suggestion on a range that ends on a removed line.";
  return undefined;
};

/** A `suggestion` block holding the lines the target covers, as they are at the head. */
export const suggestionOf = (target: Target): string => {
  const lines = target.kind === "lines" ? target.lines.filter((one) => one.kind !== "removed") : [];
  return "```suggestion\n" + lines.map((one) => one.text).join("\n") + "\n```\n";
};

export const bytes = (text: string) => new TextEncoder().encode(text).length;

const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

class Proto {
  variant = $state<Variant>("A");
  simulate = $state<Simulate>("normal");
  /** What the last action would have sent to GitHub, shown on the floating bar. */
  note = $state("");

  /** The PR the Files tab shows. */
  files = $state<PrFilesState>();
  number = $state(0);
  author = $state<string | null>(null);
  head = $state("");

  composer = $state<Composer>();
  /** Unsent text, by composer place (#366). */
  readonly unsent = new SvelteMap<string, string>();
  drafts = $state<Draft[]>([]);
  #next = 1;
  /** Submit's choice and its body, which is Unsent text too. */
  event = $state<ReviewEvent>("COMMENT");
  submitBody = $state("");
  submitState = $state<{ kind: "editing" } | { kind: "sending" } | { kind: "gone" } | { kind: "not_sent" }>({
    kind: "editing",
  });
  submitOpen = $state(false);
  /** B's panel tab. */
  panelTab = $state<"comment" | "review">("review");
  submitted = $state<Submitted[]>([
    { event: "COMMENT", commit: "3f9a0c1", comments: 2, at: new Date(Date.now() - 3 * 86_400_000) },
  ]);
  localMark = $state<string>();
  /** The rows of the range the user drags or shift-clicks. */
  anchorRow = $state<number>();
  dragging = $state(false);

  readonly readOnly = $derived(this.simulate === "read_only");
  readonly own = $derived(this.simulate === "own_pr");
  readonly closed = $derived(this.simulate === "closed");
  /** Whether a Pending review is open on GitHub: once it holds a draft, or begun on github.com. */
  readonly pending = $derived(this.drafts.length > 0);
  readonly commentCount = $derived(this.drafts.length);

  setVariant(variant: Variant) {
    this.variant = variant;
    this.submitOpen = false;
  }

  setSimulate(simulate: Simulate) {
    this.simulate = simulate;
    if (simulate === "github_pending") this.#seedFromGitHub();
    if (simulate === "placed_newer") {
      const first = this.drafts.find((one) => one.target.kind === "lines");
      if (first) first.checkLine = true;
      else this.say("Add a line comment first, then pick this again to mark it");
    }
  }

  say(note: string) {
    this.note = note;
  }

  /** Two drafts from a Pending review begun on github.com, on the first added rows the tab has. */
  #seedFromGitHub() {
    const files = this.files;
    if (!files || this.drafts.some((one) => one.fromGitHub)) return;
    let found = 0;
    for (let row = 0; row < files.layout.count && found < 2; row++) {
      const one = files.row(row);
      if (one?.kind !== "added") continue;
      const path = this.pathOf(row);
      if (path === undefined) continue;
      this.drafts.push({
        id: this.#next++,
        target: { kind: "lines", path, lines: [{ row, side: "RIGHT", line: one.new, text: one.text, kind: "added" }] },
        body: found === 0 ? "Begun on github.com: is this still needed?" : "Begun on github.com: nit, a typo here.",
        fromGitHub: true,
        checkLine: false,
      });
      found++;
      row += 30;
    }
    this.say(found ? "Found your Pending review on github.com with 2 drafts" : "Scroll the diff so rows load, then pick again");
  }

  bind(files: PrFilesState, number: number, author: string | null, head: string) {
    if (this.number !== number) {
      this.composer = undefined;
      this.drafts = [];
      this.localMark = undefined;
      this.submitBody = "";
    }
    this.files = files;
    this.number = number;
    this.author = author;
    this.head = head;
  }

  pathOf(row: number): string | undefined {
    const files = this.files;
    if (!files || files.view?.state !== "ready") return undefined;
    const at = files.layout.fileAt(row);
    return at === undefined ? undefined : files.view.files[at]?.path;
  }

  lineAt(row: number): Line | undefined {
    const one = this.files?.row(row);
    switch (one?.kind) {
      case "added":
        return { row, side: "RIGHT", line: one.new, text: one.text, kind: "added" };
      case "removed":
        return { row, side: "LEFT", line: one.old, text: one.text, kind: "removed" };
      case "context":
        return { row, side: "RIGHT", line: one.new, text: one.text, kind: "context" };
      default:
        return undefined;
    }
  }

  /** The rows from `from` to `to`, if they are lines of one hunk. */
  span(from: number, to: number): Line[] | undefined {
    const [low, high] = from <= to ? [from, to] : [to, from];
    const lines: Line[] = [];
    for (let row = low; row <= high; row++) {
      const one = this.files?.row(row);
      if (one?.kind === "note") continue;
      const line = this.lineAt(row);
      if (!line) return undefined;
      lines.push(line);
    }
    return lines;
  }

  /** A click on a line's number: a comment on that line, or with Shift, the range to it from the anchor. */
  pick(row: number, extend: boolean) {
    if (this.readOnly && !this.composer) this.say("Read-only: the composer still works, and offers Copy and open on GitHub");
    const path = this.pathOf(row);
    if (path === undefined) return;
    const from = extend && this.anchorRow !== undefined ? this.anchorRow : row;
    const lines = this.span(from, row);
    if (!lines) {
      this.say("A comment covers lines of one hunk (scope.md), so the range stops at the hunk's edge");
      return;
    }
    if (!extend) this.anchorRow = row;
    this.open({ kind: "lines", path, lines });
  }

  /** Grows or shrinks the range from the composer, by keys: `edge` is the end that moves. */
  stretch(edge: "top" | "bottom", by: 1 | -1) {
    const target = this.composer?.target;
    if (target?.kind !== "lines") return;
    const first = target.lines[0]?.row ?? 0;
    const last = target.lines.at(-1)?.row ?? 0;
    let [low, high] = [first, last];
    const step = (row: number, dir: number) => {
      let next = row + dir;
      while (this.files?.row(next)?.kind === "note") next += dir;
      return next;
    };
    if (edge === "top") low = step(low, -by);
    else high = step(high, by);
    if (low > high) return;
    const lines = this.span(low, high);
    if (!lines) {
      this.say("The range stops at the hunk's edge");
      return;
    }
    const before = placeKey(target);
    const body = this.composer?.body ?? "";
    target.lines = lines;
    this.unsent.delete(before);
    this.unsent.set(placeKey(target), body);
    this.anchorRow = edge === "top" ? high : low;
  }

  open(target: Target, editing?: Draft) {
    // A composer takes its text to another place only when it is placed again after "These lines changed
    // in the newer push".
    const carried = this.composer?.state.kind === "moved" ? this.composer.body : undefined;
    this.composer = {
      target,
      body: editing?.body ?? carried ?? this.unsent.get(placeKey(target)) ?? "",
      state: { kind: "editing" },
      editing: editing?.id,
    };
    this.panelTab = "comment";
  }

  edit(draft: Draft) {
    this.open(draft.target, draft);
  }

  type(body: string) {
    if (!this.composer) return;
    this.composer.body = body;
    if (this.composer.editing === undefined) this.unsent.set(placeKey(this.composer.target), body);
  }

  close() {
    this.composer = undefined;
    this.anchorRow = undefined;
    this.panelTab = "review";
  }

  /** Esc: closes the composer and keeps its text for that place. */
  cancel() {
    if (this.composer?.body.trim()) this.say("Closed. The text stays as Unsent text and comes back on that line");
    this.close();
  }

  /** Waits as a write would, then says what the bar pretends: the outcome of the write. */
  async #write(): Promise<"ok" | "not_sent" | "unknown" | "moved" | "refused"> {
    await wait(this.simulate === "slow" ? 3000 : 600);
    const outcome = this.simulate;
    if (outcome === "not_sent" || outcome === "unknown" || outcome === "moved" || outcome === "refused") {
      this.simulate = outcome === "refused" ? "read_only" : "normal";
      return outcome;
    }
    return "ok";
  }

  /** Add to review, or Comment now: the one comment goes out alone, as a review submitted at once. */
  async send(alone = false) {
    const composer = this.composer;
    if (!composer || !composer.body.trim() || composer.state.kind === "sending") return;
    if (composer.state.kind === "unknown") return;
    composer.state = { kind: "sending" };
    const outcome = await this.#write();
    if (this.composer !== composer) return;
    switch (outcome) {
      case "not_sent":
        composer.state = { kind: "not_sent", why: "GitHub answered 502 Bad Gateway" };
        return;
      case "unknown":
        composer.state = { kind: "unknown" };
        return;
      case "moved":
        composer.state = { kind: "moved" };
        return;
      case "refused":
        composer.state = { kind: "refused" };
        this.say("GitHub refused the write: this Repository is Read-only until the token changes");
        return;
    }
    this.#land(composer, alone);
  }

  /** "Check now": pretends the connection came back and the read-back found the comment there. */
  async check() {
    const composer = this.composer;
    if (composer?.state.kind !== "unknown") return;
    composer.state = { kind: "sending" };
    await wait(600);
    this.say("Read back: GitHub has the comment, so it isn't sent again");
    this.#land(composer, false);
  }

  #land(composer: Composer, alone: boolean) {
    const target = composer.target;
    this.unsent.delete(placeKey(target));
    if (target.kind === "conversation") {
      this.say(`Would call addComment: posted at once to ${target.label}`);
      this.close();
      return;
    }
    if (composer.editing !== undefined) {
      const draft = this.drafts.find((one) => one.id === composer.editing);
      if (draft) draft.body = composer.body;
      this.say("Would call updatePullRequestReviewComment");
      this.close();
      return;
    }
    if (alone) {
      this.submitted.push({ event: "COMMENT", commit: this.head.slice(0, 7), comments: 1, at: new Date() });
      this.say("Would call addPullRequestReview with event COMMENT and this one thread: sent alone");
      this.close();
      return;
    }
    const opened = !this.pending;
    this.drafts.push({ id: this.#next++, target, body: composer.body, fromGitHub: false, checkLine: false });
    const call = target.kind === "reply" ? "addPullRequestReviewThreadReply" : "addPullRequestReviewThread";
    this.say(`Would call ${call}${opened ? ", which opens your Pending review" : ", into your Pending review"}`);
    this.close();
  }

  remove(draft: Draft) {
    this.drafts = this.drafts.filter((one) => one.id !== draft.id);
    this.say(
      this.drafts.length === 0
        ? "Would call deletePullRequestReviewComment. The Pending review stays open on GitHub, empty"
        : "Would call deletePullRequestReviewComment",
    );
  }

  discard() {
    this.drafts = [];
    this.submitOpen = false;
    this.say("Would call deletePullRequestReview: the Pending review and its drafts are gone");
  }

  /** Why Submit can't go, or undefined when it can. */
  readonly submitBlocked = $derived.by((): string | undefined => {
    if (this.readOnly) return "Read-only: your token can't write to this Repository. Submit the review on GitHub.";
    if (this.own && this.event !== "COMMENT")
      return "GitHub doesn't let you approve or request changes on your own PR.";
    if (this.event === "COMMENT" && !this.submitBody.trim() && this.drafts.length === 0)
      return "A Comment needs a summary or at least one draft.";
    if (this.event === "REQUEST_CHANGES" && !this.submitBody.trim() && this.drafts.length === 0)
      return "Say what to change: a summary or at least one draft.";
    return undefined;
  });

  async submit() {
    if (this.submitBlocked || this.submitState.kind === "sending") return;
    this.submitState = { kind: "sending" };
    await wait(this.simulate === "slow" ? 3000 : 700);
    if (this.simulate === "submitted_elsewhere") {
      this.simulate = "normal";
      this.drafts = [];
      this.submitState = { kind: "gone" };
      return;
    }
    const comments = this.drafts.length;
    this.submitted.push({ event: this.event, commit: this.head.slice(0, 7), comments, at: new Date() });
    this.say(
      `Would call submitPullRequestReview with event ${this.event}${this.pending ? "" : " (addPullRequestReview: no review was open)"}`,
    );
    this.drafts = [];
    this.submitBody = "";
    this.event = "COMMENT";
    this.submitState = { kind: "editing" };
    this.submitOpen = false;
  }

  toggleMark() {
    if (!this.head) return;
    const head = this.head.slice(0, 7);
    if (this.localMark === head) {
      this.localMark = undefined;
      this.say("Local mark removed");
    } else {
      this.localMark = head;
      this.say(`Local mark at ${head}: only Tsugiki knows`);
    }
  }

  jumpTo(target: Target) {
    if (!this.files) return;
    if (target.kind === "reply") this.files.jump = { id: target.thread };
    const row = rowOf(target);
    if (row !== undefined) this.files.jump = { row };
  }
}

export const proto = new Proto();

/** The row a draft or composer sits under in the diff, if it sits under one. */
export const rowOf = (target: Target): number | undefined => {
  switch (target.kind) {
    case "lines":
      return target.lines.at(-1)?.row;
    case "file":
      return target.row;
    case "reply":
    case "conversation":
      return undefined;
  }
};

export const eventWords = (event: ReviewEvent) =>
  event === "APPROVE" ? "Approved" : event === "REQUEST_CHANGES" ? "Changes requested" : "Commented";
