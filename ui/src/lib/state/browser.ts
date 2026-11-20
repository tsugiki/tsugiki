// Sends pages to the system browser. Tsugiki never navigates its own window to a web page
// (docs/devel/links.md).
import { commands, type GitHubItem } from "$lib/data/bindings";
import { problemIn } from "./answer";

export type { GitHubItem };

/** Opens an Issue or a PR on GitHub, and says what went wrong if it couldn't. */
export const openOnGitHub = (repository: string, item: GitHubItem) =>
  problemIn(commands.openOnGithub(repository, item));

/** Opens a link from a description or a comment, and says what went wrong if it couldn't. */
export const openLink = (address: string) => problemIn(commands.openLink(address));
