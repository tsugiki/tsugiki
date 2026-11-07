// The Issue on show in the Detail pane.
import {
  commands,
  type HiddenReason as HiddenReasonData,
  type IssueComment_Serialize,
  type IssueContent_Serialize,
  type IssueView_Serialize,
} from "$lib/data/bindings";
import { DetailState } from "./detail-view.svelte";

export type IssueComment = IssueComment_Serialize;
/** Why someone hid a comment on GitHub. */
export type HiddenReason = HiddenReasonData;
/** The description and recent comments of an Issue or a PR. */
export type IssueContent = IssueContent_Serialize;
export type IssueView = IssueView_Serialize;

export class IssueViewState extends DetailState<IssueView> {
  constructor(repository: string, number: number) {
    super(repository, number, {
      open: (repository, number) => commands.openIssue(repository, number),
      fetch: (repository, number) => commands.fetchIssue(repository, number),
      read: (repository, number) => commands.issue(repository, number),
    });
  }
}
