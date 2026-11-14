// Where the token comes from, in words, for onboarding and Settings › Account (docs/devel/token.md).
import type { TokenSource } from "$lib/state/home.svelte";

export const TOKEN_SOURCE_WORDS: Readonly<Record<TokenSource, string>> = {
  gh: "Your token comes from the GitHub CLI (gh auth token).",
};
