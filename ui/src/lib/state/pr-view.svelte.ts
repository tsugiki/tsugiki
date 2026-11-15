// The PR on show in the Detail pane, on its Conversation or its Files tab, and the rail's Search of its
// code.
import {
  commands,
  type PrView_Serialize,
  type PullRequestContent_Serialize,
  type ReviewState as ReviewStateData,
  type Review_Serialize,
  type ThreadLine_Serialize,
} from "$lib/data/bindings";
import { DetailState } from "./detail-view.svelte";
import type { FileView } from "./file-view.svelte";
import { hud } from "./hud.svelte";
import type { PanesState } from "./panes.svelte";
import type { PeekLayout } from "./peek";
import { DiffViewChoice, PrFilesState, type FilesMode } from "./pr-files.svelte";
import { type Match, TextSearchState } from "./text-search.svelte";
import type { Stop } from "./workbench.svelte";

export type PrView = PrView_Serialize;
/** A PR's description, recent comments and Reviews. */
export type PullRequestContent = PullRequestContent_Serialize;
export type Review = Review_Serialize;
export type ReviewState = ReviewStateData;
export type ThreadLine = ThreadLine_Serialize;
export type PrTab = "conversation" | "files";

export class PrViewState extends DetailState<PrView> {
  /** The Issue the user opened this PR from, which Esc and its `gh-N` link go back to. */
  readonly issue: number | undefined;
  /** Its Files tab, which reads the diff the first time it shows. Opening the PR has already handed a head
   * the mirror lacks to the mirror. */
  readonly files: PrFilesState;
  /** The rail's Search, whose query and toggles stay while the view lasts (#686). */
  readonly search: TextSearchState;
  /** A PR opens on its Conversation. */
  #tab = $state<PrTab>("conversation");
  #filesLoaded = false;

  /** `panes` is the layout of the panes the Repository's page read, which holds the Definition peek's
   * height, `choice` the view of the diff, which the page keeps from PR to PR, `mode` whether the Files
   * tab opens on all the files or one at a time, as the settings say, and `fileViewMoved` hears of each file
   * view the user opens or closes over the Files tab, for the Detail pane's history (#456), and `peekLayout`
   * what `[diff] peek` says as each Definition peek opens (#459). */
  constructor(
    repository: string,
    number: number,
    issue: number | undefined,
    panes: PanesState,
    choice: DiffViewChoice = new DiffViewChoice(),
    mode: FilesMode = "all",
    fileViewMoved?: (view: FileView | undefined) => void,
    peekLayout?: () => PeekLayout,
  ) {
    super(repository, number, {
      open: (repository, number) => commands.openPullRequest(repository, number),
      fetch: (repository, number) => commands.fetchPullRequest(repository, number),
      read: (repository, number) => commands.pullRequest(repository, number),
    });
    this.issue = issue;
    this.files = new PrFilesState(repository, number, panes, hud, choice, mode, fileViewMoved, peekLayout);
    this.search = new TextSearchState(repository);
  }

  /** Hands the Files tab the user's Review, which marks and collapses its files while it's a Stale review
   * (#387). */
  protected override heard(view: PrView | null) {
    this.files.followReview(view?.your_review);
  }

  get tab(): PrTab {
    return this.#tab;
  }

  set tab(tab: PrTab) {
    this.#tab = tab;
    if (tab === "files") this.loadFiles();
  }

  /** Reads the Files tab's diff, unless it has been read: the rail's Search asks for it on either tab, as
   * each query names the ends of the diff on show (#686). */
  loadFiles() {
    if (this.#filesLoaded) return;
    this.#filesLoaded = true;
    void this.files.load();
  }

  override stop() {
    super.stop();
    this.files.stop();
    this.search.stop();
  }

  /** Opens `match`, which the rail's Search found, in the Files tab: at its line in the diff, or in the file
   * view of a file the PR doesn't change (#686). */
  async openMatch(match: Match) {
    this.tab = "files";
    await this.files.openMatch(match.path, match.line, match.ends);
  }

  /** The line of the list the user opened this PR from. A PR that fixes two Issues has a line under each. */
  get line(): Stop {
    return { number: this.number, kind: "pull_request", issue: this.issue };
  }

  /** Opens review thread `id` in the Files tab, unfolded (#184). */
  openThread(id: string) {
    this.tab = "files";
    this.files.reveal(id);
  }

  /** Opens the Files tab at the first file changed since the user's Stale review, as the banner's Review
   * changes button does (#387). */
  reviewChanges() {
    this.tab = "files";
    this.files.showChanges();
  }

  /** Moves to the other tab. With two tabs, the next one and the one before are the same. */
  switchTab() {
    this.tab = this.tab === "conversation" ? "files" : "conversation";
  }
}
