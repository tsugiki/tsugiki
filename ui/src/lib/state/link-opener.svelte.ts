// What a part of the window opens in the system browser, and what kept the browser from opening it.
import { type GitHubItem, openLink, openOnGitHub } from "./browser";

/** Opens pages in the system browser for one part of the window, which shows what went wrong with the last
 * one until the next. */
export class LinkOpener {
  /** What kept the browser from opening the last page asked for, if anything did. */
  problem = $state<string>();

  /** Opens `repository`'s Issue or PR `item` on GitHub. */
  async openOnGitHub(repository: string, item: GitHubItem) {
    this.problem = await openOnGitHub(repository, item);
  }

  /** Opens `address`, a link from a description, a comment or the window's own words. */
  async openLink(address: string) {
    this.problem = await openLink(address);
  }

  /** Forgets the last problem, as when what met it has gone. */
  forget() {
    this.problem = undefined;
  }
}
