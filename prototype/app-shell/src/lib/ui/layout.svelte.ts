// PROTOTYPE — throwaway. Pane sizes and the Sidebar's state for this window. They outlive a Repository
// switch; the real app would keep them per machine, like a window's position, not in config.toml.
export const SIDEBAR = { def: 230, min: 170, max: 420, rail: 48, /** narrower than this snaps to icons */ snap: 120 };
export const LIST = { def: 430, min: 320 };
/** The Detail pane never gets narrower than this; the Issue list gives way first. */
export const DETAIL_MIN = 420;

export type SidebarMode = "full" | "icons" | "hidden";
export const layout = $state<{ sidebar: number; list: number; mode: SidebarMode; shown: Exclude<SidebarMode, "hidden"> }>({
  sidebar: SIDEBAR.def, list: LIST.def, mode: "full", shown: "full",
});

/** Ctrl+B: hide the Sidebar, or bring it back as it was (full or icons). */
export function toggleSidebar() {
  if (layout.mode === "hidden") layout.mode = layout.shown;
  else { layout.shown = layout.mode; layout.mode = "hidden"; }
}
export function setSidebar(mode: SidebarMode) {
  if (mode !== "hidden") layout.shown = mode;
  layout.mode = mode;
}
export const sidebarWidth = () => (layout.mode === "full" ? layout.sidebar : layout.mode === "icons" ? SIDEBAR.rail : 0);
