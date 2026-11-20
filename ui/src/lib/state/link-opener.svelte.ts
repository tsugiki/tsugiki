// What a part of the window opens in the system browser, and what kept the browser from opening it.
import { type GitHubItem, openLink, openOnGitHub } from "./browser";

/** Opens pages in the system browser for one part of the window, which shows what went wrong with the last
 * one until the next. */
export class LinkOpener {
  /** What kept the browser from opening the last page asked for, if anything did. */
  problem = $state<string>();
  /** Counts the pages asked for and the forgets, so an answer that a newer one followed goes unheard. */
  #asked = 0;

  /** Opens `repository`'s Issue or PR `item` on GitHub. */
  openOnGitHub(repository: string, item: GitHubItem) {
    return this.#hear(openOnGitHub(repository, item));
  }

  /** Opens `address`, a link from a description, a comment or the window's own words. */
  openLink(address: string) {
    return this.#hear(openLink(address));
  }

  /** Forgets the last problem, and any answer still on its way, as when what met it has gone. */
  forget() {
    this.#asked += 1;
    this.problem = undefined;
  }

  async #hear(opening: Promise<string | undefined>) {
    this.#asked += 1;
    const asked = this.#asked;
    const problem = await opening;
    if (asked === this.#asked) this.problem = problem;
  }
}
