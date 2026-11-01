// PROTOTYPE — throwaway. Platform: what the desktop shell does for the app. Under Tauri a URL opens in
// the system browser through the opener plugin; in a plain browser tab (npm run dev) in a new tab.
import { openUrl } from "@tauri-apps/plugin-opener";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function openInBrowser(url: string) {
  if (inTauri()) void openUrl(url);
  else window.open(url, "_blank", "noopener");
}
