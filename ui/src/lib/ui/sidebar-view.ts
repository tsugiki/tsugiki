// The view on show, as both shapes of the Sidebar mark it: the full Sidebar's lines and its strip of icons
// (#587).
import type { SidebarView } from "$lib/state/workbench.svelte";

/** The `aria-current` of the Sidebar's entry for `view` while `selected` is on show: "true" for the same
 * view, a Filter by its search, a person Filter by its login, and none for another. */
export function currentView(selected: SidebarView, view: SidebarView): "true" | undefined {
  const same =
    view.kind === "filter"
      ? selected.kind === "filter" && selected.query === view.query
      : view.kind === "person"
        ? selected.kind === "person" && selected.login === view.login
        : selected.kind === view.kind;
  return same ? "true" : undefined;
}
