// The Issue on show in the Detail pane.
import {
  commands,
  type HiddenReason as HiddenReasonData,
  type IssueComment_Serialize,
  type IssueContent_Serialize,
  type IssueView_Serialize,
  type TreeEntry_Serialize,
} from "$lib/data/bindings";
import { DetailState } from "./detail-view.svelte";

export type IssueComment = IssueComment_Serialize;
/** Why someone hid a comment on GitHub. */
export type HiddenReason = HiddenReasonData;
/** The description and recent comments of an Issue or a PR. */
export type IssueContent = IssueContent_Serialize;
export type IssueView = IssueView_Serialize;
/** A line of an Issue's tree: an Origin PR with its Backports, or a Backport on its own. */
export type TreeEntry = TreeEntry_Serialize;

export class IssueViewState extends DetailState<IssueView> {
  /** How far down the user scrolled this view's pane, in pixels, so the pane comes back there after a PR
   * from its tree has hidden it (#296). The pane writes it as it scrolls, and reads it when it shows. It
   * isn't `$state`: the pane would then set its scroll on each scroll event, and fight a smooth scroll. */
  scrollTop = 0;
  /** Whether the Issue's fully merged PRs show, unfolded from their line (#588). It stays as the user left
   * it while a PR from the tree hides the view. */
  mergedShown = $state(false);

  toggleMerged() {
    this.mergedShown = !this.mergedShown;
  }

  constructor(repository: string, number: number) {
    super(repository, number, {
      open: (repository, number) => commands.openIssue(repository, number),
      fetch: (repository, number) => commands.fetchIssue(repository, number),
      read: (repository, number) => commands.issue(repository, number),
    });
  }
}
