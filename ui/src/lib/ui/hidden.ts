import type { HiddenReason } from "$lib/state/issue-view.svelte";

/** What a hidden comment or Review shows in its place, by why GitHub hid it. */
export const REASONS: Record<HiddenReason, string> = {
  abuse: "Hidden as abuse",
  off_topic: "Hidden as off-topic",
  outdated: "Hidden as outdated",
  resolved: "Hidden as resolved",
  duplicate: "Hidden as duplicate",
  spam: "Hidden as spam",
  unstated: "Hidden",
};
