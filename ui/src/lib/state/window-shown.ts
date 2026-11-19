// Tells the core when the window loads, hides and shows, as the core reads the Issue or PR on screen from
// GitHub again only while the user can see it (ADR 0015).
import { commands } from "$lib/data/bindings";

/** Tells the core the window loaded afresh, as the page before may have gone with items open, and whether
 * it shows, then again each time that changes, until the returned stop. Call it before any view opens. */
export function tellCoreWhenShown(): () => void {
  tellCore(commands.windowLoaded);
  const tell = () => tellCore(document.hidden ? commands.windowHidden : commands.windowShown);
  if (document.hidden) tell();
  document.addEventListener("visibilitychange", tell);
  return () => document.removeEventListener("visibilitychange", tell);
}

/** Sends `command`. A call that fails has no core to tell. */
function tellCore(command: () => Promise<void>) {
  command().catch(() => {});
}
