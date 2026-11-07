// The PR on show in the Detail pane, on its Conversation or its Files tab.
import { commands, type PrView_Serialize } from "$lib/data/bindings";
import { DetailState } from "./detail-view.svelte";
import type { Stop } from "./workbench.svelte";

export type PrView = PrView_Serialize;
export type PrTab = "conversation" | "files";

export class PrViewState extends DetailState<PrView> {
  /** A PR opens on its Conversation. */
  tab = $state<PrTab>("conversation");
  /** The Issue the user opened this PR from, which Esc and its `gh-N` link go back to. */
  readonly issue: number | undefined;

  constructor(repository: string, number: number, issue: number | undefined) {
    super(repository, number, {
      open: (repository, number) => commands.openPullRequest(repository, number),
      fetch: (repository, number) => commands.fetchPullRequest(repository, number),
      read: (repository, number) => commands.pullRequest(repository, number),
    });
    this.issue = issue;
  }

  /** The line of the list the user opened this PR from. A PR that fixes two Issues has a line under each. */
  get line(): Stop {
    return { number: this.number, kind: "pull_request", issue: this.issue };
  }

  /** Moves to the other tab. With two tabs, the next one and the one before are the same. */
  switchTab() {
    this.tab = this.tab === "conversation" ? "files" : "conversation";
  }
}
