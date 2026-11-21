import type { ReadFailure } from "$lib/state/detail-view.svelte";

/** The words a PR's Conversation tab shows while it has no Conversation: it is reading it, or why the read
 * failed. The Files tab of a PR whose head only the Conversation names says the same of a failed read. */
export const PR_UNREAD: Record<ReadFailure | "reading", string> = {
  reading: "Reading from GitHub…",
  no_token: "Tsugiki can't ask GitHub for the Conversation without a token. The status bar says why.",
  waits_on_github: "The Conversation comes once GitHub answers.",
  not_on_github: "GitHub doesn't have this PR, or your token can't see it.",
  other: "Tsugiki couldn't read this PR's Conversation from GitHub. tsugiki.log says why.",
};
