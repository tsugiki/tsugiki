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

/** The Settings dialog: moved by its title bar, resized from any edge between its minimum and the
 *  window, or maximized. x, y: its top-left corner; null until moved or resized, which means centred. */
export const DIALOG = { w: 880, h: 600, minW: 640, minH: 420 };
export const dialog = $state<{ w: number; h: number; x: number | null; y: number | null; max: boolean }>({ w: DIALOG.w, h: DIALOG.h, x: null, y: null, max: false });
